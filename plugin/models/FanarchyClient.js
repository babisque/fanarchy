.pragma library
.import "format.js" as Format

// State of the connection to fanarchyd (docs/protocol.md).
//
// QML cannot open a socket from JavaScript, so components/FanarchyConnection
// owns the Quickshell Socket and forwards every event here. All decisions
// live in these pure functions: each takes the current state and returns a
// new one, never mutating its input.

var PLUGIN_ID = "io.github.babisque.fanarchy"
var SOCKET_PATH = "/run/fanarchy.sock"
var RECONNECT_MS = 5000
var HOT_MARGIN_C = 10

function subscribeLine() {
  return JSON.stringify({ cmd: "subscribe" }) + "\n"
}

// Shell command that opens or closes the Fanarchy panel.
function togglePanelCommand() {
  return "omarchy-shell shell toggle " + PLUGIN_ID
}

// How the bar widget toggles the panel: "api" through the shell facade
// Omarchy hands the bar (bar.shell.toggle, in-process), "command" through
// the omarchy-shell CLI when that facade is missing.
function panelToggleRoute(shellApi) {
  return shellApi && typeof shellApi.toggle === "function" ? "api" : "command"
}

function initialState() {
  return { connected: false, status: null, error: null }
}

function onConnected(state) {
  return { connected: true, status: null, error: null }
}

// A dropped connection forgets the last status so stale numbers are never
// shown. `errorText` is the socket error, if any.
function onDisconnected(state, errorText) {
  var error = errorText ? String(errorText) : (state ? state.error : null)
  return { connected: false, status: null, error: error }
}

// Handle one line from the daemon. Returns { state, log }: `log` is a
// message for the shell console, or null.
function onLine(state, line) {
  var text = String(line === undefined || line === null ? "" : line).trim()
  if (text === "") return { state: state, log: null }

  var message
  try {
    message = JSON.parse(text)
  } catch (e) {
    return { state: state, log: "fanarchy: ignored a malformed line from the daemon" }
  }
  if (!message || typeof message !== "object") {
    return { state: state, log: "fanarchy: ignored a malformed line from the daemon" }
  }

  if (message.ok === true && message.data && typeof message.data === "object") {
    return {
      state: { connected: state.connected, status: message.data, error: null },
      log: null
    }
  }
  if (message.ok === false) {
    var error = typeof message.error === "string" ? message.error : "unknown daemon error"
    return {
      state: { connected: state.connected, status: state.status, error: error },
      log: "fanarchy: daemon error: " + error
    }
  }
  return { state: state, log: null }
}

function isNumber(value) {
  return typeof value === "number" && isFinite(value)
}

// "down" when the socket is unreachable, "hot" when the CPU is at or above
// critical_temp_c - 10, otherwise "normal" (including before the first
// status arrives).
function widgetState(state) {
  if (!state || !state.connected) return "down"
  var status = state.status
  if (!status) return "normal"
  var cpu = status.cpu_temp_c
  var critical = status.critical_temp_c
  if (isNumber(cpu) && isNumber(critical) && cpu >= critical - HOT_MARGIN_C) return "hot"
  return "normal"
}

// Quickshell's Socket reports errors as QLocalSocket::LocalSocketError
// numbers. Name the ones the tooltip cares about.
var SOCKET_ERRORS = {
  0: "connection refused",
  1: "peer closed",
  2: "server not found",
  3: "access denied",
  4: "resource error",
  5: "timeout"
}

function socketErrorText(code) {
  var known = SOCKET_ERRORS[code]
  return known ? known : "socket error " + code
}

// Why the daemon looks down: "permission" when the socket exists but this
// user is not in the fanarchy group, otherwise "stopped".
function downReason(errorText) {
  var text = String(errorText || "").toLowerCase()
  if (text.indexOf("permission") !== -1 || text.indexOf("access") !== -1
      || text.indexOf("denied") !== -1) {
    return "permission"
  }
  return "stopped"
}

// Rows of the tooltip grid: { name, value }.
function tooltipRows(status) {
  if (!status) return []
  var rows = []
  var gpus = Array.isArray(status.gpus) ? status.gpus : []
  for (var i = 0; i < gpus.length; i++) {
    rows.push({
      name: gpus.length > 1 ? "GPU " + (i + 1) : "GPU",
      value: Format.celsius(gpus[i] ? gpus[i].celsius : null)
    })
  }
  if (gpus.length === 0 && status.gpu_note) {
    rows.push({ name: "GPU", value: Format.celsius(null) })
  }
  var fans = Array.isArray(status.fans) ? status.fans : []
  for (var j = 0; j < fans.length; j++) {
    var fan = fans[j] || {}
    rows.push({ name: String(fan.label || fan.id || "fan"), value: Format.fanValue(fan) })
  }
  return rows
}

// Everything the bar widget and its tooltip draw, in one object.
function view(state) {
  var current = widgetState(state)
  var cpu = state && state.status ? state.status.cpu_temp_c : null
  var number = current === "down" ? Format.temperature(null) : Format.temperature(cpu)
  return {
    state: current,
    label: current === "hot" ? number + " !" : number,
    accessibleName: Format.accessibleName(current, cpu),
    rows: current === "down" ? [] : tooltipRows(state ? state.status : null),
    downReason: current === "down" ? downReason(state ? state.error : null) : null
  }
}

// ---------------------------------------------------------------- commands
//
// The panel sends requests on a second connection (a subscribed one ignores
// further requests). The daemon answers each request with exactly one line,
// in order, so pending requests are a FIFO queue.

function requestLine(request) {
  return JSON.stringify(request) + "\n"
}

function getConfigRequest() {
  return { cmd: "get_config" }
}

function setCurveRequest(fanId, points) {
  var copy = []
  for (var i = 0; i < points.length; i++) copy.push([Number(points[i][0]), Number(points[i][1])])
  return { cmd: "set_curve", fan: String(fanId), points: copy }
}

// `mode` is "curve", "fixed" or "auto". `percent` is only sent with fixed.
function setModeRequest(fanId, mode, percent) {
  var request = { cmd: "set_mode", fan: String(fanId), mode: String(mode) }
  if (mode === "fixed") request.percent = Math.round(Number(percent))
  return request
}

function enqueue(queue, request) {
  return (queue || []).concat([request])
}

// Pair one response line with the oldest pending request. Returns
// { queue, request, ok, data, error, log }; `request` is null when the
// line was blank, malformed or unexpected.
function settle(queue, line) {
  var pending = queue || []
  var text = String(line === undefined || line === null ? "" : line).trim()
  if (text === "") return { queue: pending, request: null, ok: false, data: null, error: null, log: null }

  var message = null
  try { message = JSON.parse(text) } catch (e) { message = null }
  var request = pending.length > 0 ? pending[0] : null
  var rest = pending.slice(1)

  if (!message || typeof message !== "object") {
    // Still one line for one request: drop the request so the queue stays aligned.
    return {
      queue: rest, request: request, ok: false, data: null,
      error: "the daemon sent an unreadable reply",
      log: "fanarchy: unreadable reply from the daemon"
    }
  }
  if (!request) {
    return { queue: pending, request: null, ok: false, data: null, error: null, log: "fanarchy: reply with no request" }
  }
  if (message.ok === true) {
    return { queue: rest, request: request, ok: true, data: message.data === undefined ? null : message.data, error: null, log: null }
  }
  var error = typeof message.error === "string" ? message.error : "the daemon refused the request"
  return { queue: rest, request: request, ok: false, data: null, error: error, log: null }
}

