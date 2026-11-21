.pragma library

// Formatting for every number the plugin shows. Units are always visible,
// and missing values become "—", never "NaN" or an empty string.

var MISSING = "—"

function isNumber(value) {
  return typeof value === "number" && isFinite(value)
}

// Bar number: "47°", or "—°" before the first reading.
function temperature(celsius) {
  return (isNumber(celsius) ? String(Math.round(celsius)) : MISSING) + "°"
}

// Tooltip temperature: "47 °C".
function celsius(value) {
  return (isNumber(value) ? String(Math.round(value)) : MISSING) + " °C"
}

function rpm(value) {
  return (isNumber(value) ? String(Math.round(value)) : MISSING) + " RPM"
}

// Speed the daemon applies. A pump is never controlled, and an `auto` fan
// is driven by the motherboard, so both read as AUTO.
function percent(fan) {
  if (!fan || fan.role === "pump" || !isNumber(fan.percent)) return "AUTO"
  return String(Math.round(fan.percent)) + "%"
}

function fanValue(fan) {
  return rpm(fan ? fan.rpm : null) + " · " + percent(fan)
}

// Accessible name of the bar button.
// `state` is "normal", "hot" or "down" (see FanarchyClient.widgetState).
function accessibleName(state, cpuCelsius) {
  if (state === "down") return "Fanarchy: daemon stopped"
  if (!isNumber(cpuCelsius)) return "Fanarchy: CPU temperature not available yet"
  var name = "Fanarchy: CPU " + Math.round(cpuCelsius) + " degrees"
  return state === "hot" ? name + ", running hot" : name
}
