.pragma library
.import "format.js" as Format

// What the panel shows, derived from the daemon's status and config.
// Pure functions only; Panel.qml and its components just draw the result.

var REOPEN_GUARD_MS = 400

// The three mode buttons. BIOS is the protocol's `auto` mode.
var MODES = [
  { label: "CURVE", mode: "curve" },
  { label: "FIXED", mode: "fixed" },
  { label: "BIOS", mode: "auto" }
]

function isNumber(value) {
  return typeof value === "number" && isFinite(value)
}

function controlledFans(status) {
  var fans = status && Array.isArray(status.fans) ? status.fans : []
  return fans.filter(function(fan) { return fan && fan.role === "fan" })
}

// "down": socket unreachable. "pending": connected, no status yet (show the
// layout with placeholders). "empty": no controlled fans. "ready".
function panelState(connected, status) {
  if (!connected) return "down"
  if (!status) return "pending"
  return controlledFans(status).length === 0 ? "empty" : "ready"
}

function configFan(config, id) {
  var fans = config && Array.isArray(config.fans) ? config.fans : []
  for (var i = 0; i < fans.length; i++) if (fans[i] && fans[i].id === id) return fans[i]
  return null
}

function curveOf(config, id) {
  var fan = configFan(config, id)
  return fan && Array.isArray(fan.curve) ? fan.curve : []
}

// "fan1 · pwm1 · cpu/temp1" (the fanN header and pwmN share the number).
function cardHeader(fan, fromConfig) {
  var n = fromConfig && isNumber(fromConfig.pwm) ? String(fromConfig.pwm) : Format.MISSING
  var parts = ["fan" + n, "pwm" + n]
  if (fan && fan.role !== "pump") parts.push(fan.sensor ? String(fan.sensor) : Format.MISSING)
  return parts.join(" · ")
}

function modeText(fan) {
  if (!fan) return Format.MISSING
  if (fan.override === "critical") return "CRITICAL"
  if (fan.override === "sensor_failed") return "SENSOR FAILED"
  if (fan.mode === "auto") return "bios"
  return fan.mode ? String(fan.mode) : Format.MISSING
}

function fanCard(fan, config) {
  var fromConfig = configFan(config, fan.id)
  var name = String(fan.label || fan.id || "fan").toUpperCase()
  if (fan.role === "pump") {
    return {
      id: String(fan.id), kind: "pump", name: name,
      header: "Pump · automatic control, always",
      rpm: Format.rpm(fan.rpm),
      detail: cardHeader(fan, fromConfig)
    }
  }
  return {
    id: String(fan.id), kind: "fan", name: name,
    header: cardHeader(fan, fromConfig),
    rpm: Format.rpm(fan.rpm),
    detail: Format.percent(fan) + " · " + modeText(fan)
  }
}

function fanCards(status, config) {
  var fans = status && Array.isArray(status.fans) ? status.fans : []
  var out = []
  for (var i = 0; i < fans.length; i++) if (fans[i]) out.push(fanCard(fans[i], config))
  return out
}

// Card layout before the first status: same shapes, "—" values.
function placeholderCards() {
  var dash = Format.MISSING
  return [
    { id: "", kind: "fan", name: dash, header: "fan" + dash + " · pwm" + dash + " · " + dash, rpm: Format.rpm(null), detail: dash + "% · " + dash },
    { id: "", kind: "fan", name: dash, header: "fan" + dash + " · pwm" + dash + " · " + dash, rpm: Format.rpm(null), detail: dash + "% · " + dash }
  ]
}

// Keep the current selection while that fan exists, otherwise pick the
// first controlled fan. Pumps are never selected.
function pickSelection(status, currentId) {
  var fans = controlledFans(status)
  for (var i = 0; i < fans.length; i++) if (fans[i].id === currentId) return currentId
  return fans.length > 0 ? String(fans[0].id) : ""
}

function statusFan(status, id) {
  var fans = status && Array.isArray(status.fans) ? status.fans : []
  for (var i = 0; i < fans.length; i++) if (fans[i] && fans[i].id === id) return fans[i]
  return null
}

function headerTemps(status) {
  var gpus = status && Array.isArray(status.gpus) ? status.gpus : []
  return {
    cpu: Format.temperature(status ? status.cpu_temp_c : null),
    gpu: Format.temperature(gpus.length > 0 && gpus[0] ? gpus[0].celsius : null)
  }
}

function editorTitle(fan) {
  return "CURVE — " + (fan ? String(fan.label || fan.id).toUpperCase() : Format.MISSING)
}

// Starting value of the FIXED slider: the current speed, within the fan's
// minimum and 100.
function fixedStart(fan) {
  var floor = fan && isNumber(fan.min_percent) ? fan.min_percent : 0
  var value = fan && isNumber(fan.percent) ? fan.percent : 50
  return Math.round(Math.min(100, Math.max(floor, value)))
}

function minPercent(fan) {
  return fan && isNumber(fan.min_percent) ? fan.min_percent : 0
}

// The subtle confirmation shown after the daemon accepts a request.
function confirmation(request) {
  if (!request) return ""
  if (request.cmd === "set_curve") return "✓ curve applied"
  if (request.cmd === "set_mode") {
    if (request.mode === "fixed") return "✓ fixed at " + request.percent + "%"
    if (request.mode === "auto") return "✓ BIOS in control"
    return "✓ following the curve"
  }
  if (request.cmd === "copy") return "✓ copied"
  return ""
}

// A widget click that lands right after an outside click closed the panel
// is the same gesture: don't reopen.
function ignoreReopen(lastDismissMs, nowMs) {
  return isNumber(lastDismissMs) && nowMs - lastDismissMs >= 0 && nowMs - lastDismissMs < REOPEN_GUARD_MS
}

// Copy for EmptyState.qml.
function emptyState(kind, downReason) {
  if (kind === "empty") {
    return {
      headline: "NOTHING TO REBEL AGAINST. YET.",
      line: "No fans are under your control. Scan the hardware, name your fans and mark the pump.",
      command: "fanarchyctl init",
      button: "START SETUP"
    }
  }
  if (downReason === "permission") {
    return {
      headline: "THE DAEMON WON'T LET YOU IN.",
      line: "No access to /run/fanarchy.sock. Join the fanarchy group, then log in again.",
      command: "sudo usermod -aG fanarchy $USER",
      button: "COPY COMMAND"
    }
  }
  return {
    headline: "THE DAEMON STOPPED LISTENING.",
    line: "Your fans are safe: the motherboard took control back.",
    command: "sudo systemctl start fanarchyd",
    button: "COPY COMMAND"
  }
}

function cardsOfKind(cards, kind) {
  return (cards || []).filter(function(card) { return card && card.kind === kind })
}

// Mode to highlight: the one just requested until the daemon reports it,
// otherwise the fan's reported mode.
function displayMode(fan, pending) {
  if (pending && fan && pending.fan === fan.id) return pending.mode
  return fan && fan.mode ? String(fan.mode) : ""
}

// The pending mode is done once the status reports it.
function pendingSettled(fan, pending) {
  return !pending || !fan || pending.fan !== fan.id || fan.mode === pending.mode
}

// Config copy with one fan's curve replaced (after set_curve succeeds).
function withCurve(config, fanId, points) {
  if (!config || !Array.isArray(config.fans)) return config
  var next = JSON.parse(JSON.stringify(config))
  for (var i = 0; i < next.fans.length; i++) {
    if (next.fans[i] && next.fans[i].id === fanId) next.fans[i].curve = JSON.parse(JSON.stringify(points))
  }
  return next
}

function footerText(connected) {
  return (connected ? "daemon online" : "daemon offline") + " · /run/fanarchy.sock"
}
