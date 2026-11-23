.pragma library
.import "format.js" as Format
.import "curve.js" as Curve

// The setup wizard (specs/08-setup-wizard.md): mark the pump, pick the
// fans, check and apply. Pure functions only: each takes the draft and
// returns a new one, never changing its input. SetupWizard.qml just draws
// the result.
//
// The draft:
//   { step: 1 | 2 | 3, noPump: bool, rows: [row] }
// One row per output from `list_outputs`, keyed "<chip alias>/<pwm>":
//   { key, chip, pwm, path, rpm, defaultRpmInput,
//     pump, locked,                 // marked as pump; locked when root set it
//     control, existing,            // controlled; already a fan in the config
//     label, id, idEdited, sensor, minPercent, minLow, minHigh,
//     rpmInput, curve, advanced }
//
// Nothing here talks to the daemon: steps 1 and 2 never write anything.

var STEPS = 3
var TITLES = ["MARK THE PUMP", "PICK YOUR FANS", "CHECK AND APPLY"]
var MIN_SLIDER = 10
var MAX_SLIDER = 60
var DEFAULT_MIN = 20
var DEFAULT_SENSOR = "cpu/temp1"
var ID_PATTERN = /^[a-z0-9-]+$/
// How often the wizard asks for fresh readings (the daemon ticks every second).
var REFRESH_MS = 2000

function isNumber(value) {
  return typeof value === "number" && isFinite(value)
}

function clone(value) {
  return JSON.parse(JSON.stringify(value === undefined ? null : value))
}

function listOf(value) {
  return Curve.listOf(value) || []
}

// "Case fans" -> "case-fans". Lowercase letters, digits and "-" only.
function slug(label) {
  return String(label === undefined || label === null ? "" : label)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
}

// `base`, or `base-2`, `base-3`, ... when it is taken. Falls back to
// "fan" for an empty base.
function uniqueId(base, taken) {
  var root = base || "fan"
  var used = listOf(taken)
  if (used.indexOf(root) === -1) return root
  for (var n = 2; ; n++) {
    var candidate = root + "-" + n
    if (used.indexOf(candidate) === -1) return candidate
  }
}

function outputKey(output) {
  return String(output.chip) + "/" + String(output.pwm)
}

function outputName(row) {
  return "pwm" + row.pwm + " · " + row.chip
}

function stepHeader(step) {
  return "STEP " + step + " / " + STEPS
}

function stepTitle(step) {
  return TITLES[step - 1] || ""
}

function configFans(config) {
  return config ? listOf(config.fans) : []
}

function configEntry(config, chip, pwm) {
  var fans = configFans(config)
  for (var i = 0; i < fans.length; i++) {
    if (fans[i] && fans[i].chip === chip && fans[i].pwm === pwm) return fans[i]
  }
  return null
}

// Keep a minimum the config already has, even outside the slider range.
function minRange(minPercent) {
  return {
    low: Math.min(MIN_SLIDER, minPercent),
    high: Math.max(MAX_SLIDER, minPercent)
  }
}

function newRow(output, config) {
  var assigned = output.assigned || { kind: "none", id: null }
  var entry = assigned.kind !== "none" ? configEntry(config, output.chip, output.pwm) : null
  var row = {
    key: outputKey(output),
    chip: String(output.chip),
    pwm: Number(output.pwm),
    path: String(output.path || ""),
    rpm: isNumber(output.rpm) ? output.rpm : null,
    defaultRpmInput: isNumber(output.rpm_input) ? output.rpm_input : null,
    pump: false,
    locked: false,
    control: false,
    existing: false,
    label: "",
    id: "",
    idEdited: false,
    sensor: DEFAULT_SENSOR,
    minPercent: DEFAULT_MIN,
    minLow: MIN_SLIDER,
    minHigh: MAX_SLIDER,
    rpmInput: isNumber(output.rpm_input) ? output.rpm_input : null,
    curve: null,
    advanced: false,
    saved: null
  }
  if (assigned.kind === "pump") {
    row.pump = true
    row.locked = true
    row.id = String(assigned.id || "")
    row.label = entry ? String(entry.label) : row.id
    row.idEdited = true
  } else if (assigned.kind === "fan" && entry) {
    var min = isNumber(entry.min_percent) ? entry.min_percent : DEFAULT_MIN
    var range = minRange(min)
    row.control = true
    row.existing = true
    row.label = String(entry.label)
    row.id = String(entry.id)
    row.idEdited = true
    row.sensor = entry.sensor ? String(entry.sensor) : DEFAULT_SENSOR
    row.minPercent = min
    row.minLow = range.low
    row.minHigh = range.high
    if (isNumber(entry.rpm_input)) row.rpmInput = entry.rpm_input
    row.curve = Curve.copy(entry.curve)
  }
  return row
}

// A fresh draft from a `list_outputs` reply and the running config. It
// always starts at step 1, the pump.
function initialDraft(outputs, config) {
  var list = listOf(outputs)
  var rows = []
  for (var i = 0; i < list.length; i++) if (list[i]) rows.push(newRow(list[i], config))
  return { step: 1, noPump: false, rows: rows }
}

// Refresh the live readings, keeping every choice. New outputs are added
// at the end, outputs that disappeared stay (the daemon will refuse them).
function mergeOutputs(draft, outputs, config) {
  var next = clone(draft)
  var list = listOf(outputs)
  for (var i = 0; i < list.length; i++) {
    var output = list[i]
    if (!output) continue
    var row = findRow(next, outputKey(output))
    if (row) {
      row.rpm = isNumber(output.rpm) ? output.rpm : null
      row.path = String(output.path || row.path)
    } else {
      next.rows.push(newRow(output, config))
    }
  }
  return next
}

function findRow(draft, key) {
  var rows = draft ? draft.rows : []
  for (var i = 0; i < rows.length; i++) if (rows[i].key === key) return rows[i]
  return null
}

// Ids in use by pumps and controlled fans, except the row `exceptKey`.
function takenIds(draft, exceptKey) {
  var ids = []
  for (var i = 0; i < draft.rows.length; i++) {
    var row = draft.rows[i]
    if (row.key === exceptKey) continue
    if (row.pump || row.control) ids.push(row.id)
  }
  return ids
}

// ---------------------------------------------------------------- step 1

function hasLockedPump(draft) {
  return draft.rows.some(function(row) { return row.locked })
}

function pumps(draft) {
  return draft.rows.filter(function(row) { return row.pump })
}

// Mark or unmark an output as a pump. Pumps from the config are locked.
function togglePump(draft, key) {
  var next = clone(draft)
  var row = findRow(next, key)
  if (!row || row.locked) return next
  row.pump = !row.pump
  if (row.pump) {
    row.saved = { control: row.control, label: row.label, id: row.id, idEdited: row.idEdited }
    row.control = false
    row.id = uniqueId("pump", takenIds(next, key))
    row.label = pumpLabel(row.id)
    row.idEdited = false
    next.noPump = false
  } else {
    restoreFanFields(row)
  }
  return next
}

// The label that goes with a proposed pump id: "pump" -> "Pump",
// "pump-2" -> "Pump 2", so two pumps never show the same name.
function pumpLabel(id) {
  var match = /^pump-(\d+)$/.exec(String(id))
  return match ? "Pump " + match[1] : "Pump"
}

// What the row was before it was marked as a pump.
function restoreFanFields(row) {
  var saved = row.saved || { control: false, label: "", id: "", idEdited: false }
  row.control = saved.control
  row.label = saved.label
  row.id = saved.id
  row.idEdited = saved.idEdited
  row.saved = null
}

// "I HAVE NO PUMP": unmark every pump that is not locked.
function chooseNoPump(draft) {
  var next = clone(draft)
  for (var i = 0; i < next.rows.length; i++) {
    var row = next.rows[i]
    if (row.pump && !row.locked) {
      row.pump = false
      restoreFanFields(row)
    }
  }
  next.noPump = true
  return next
}

function canLeaveStep1(draft) {
  return !!draft && (draft.noPump || pumps(draft).length > 0)
}

// ---------------------------------------------------------------- step 2

function defaultSensor(options) {
  var list = listOf(options)
  for (var i = 0; i < list.length; i++) if (list[i].value === DEFAULT_SENSOR) return DEFAULT_SENSOR
  return list.length > 0 ? list[0].value : DEFAULT_SENSOR
}

// Turn control of an output on or off. Turning it on fills the defaults:
// label "Fan N", id from the label, the default sensor and 20 % minimum.
function toggleControl(draft, key, sensor) {
  var next = clone(draft)
  var row = findRow(next, key)
  if (!row || row.pump) return next
  row.control = !row.control
  if (row.control && !row.label) {
    row.label = "Fan " + row.pwm
    row.idEdited = false
    row.sensor = sensor || DEFAULT_SENSOR
  }
  if (row.control && !row.idEdited) row.id = uniqueId(slug(row.label), takenIds(next, key))
  return next
}

// Change one field of a row. While the id was never edited by hand it
// follows the label.
function setField(draft, key, field, value) {
  var next = clone(draft)
  var row = findRow(next, key)
  if (!row) return next
  if (field === "label") {
    row.label = String(value)
    if (!row.idEdited) row.id = uniqueId(slug(row.label), takenIds(next, key))
  } else if (field === "id") {
    row.id = String(value)
    row.idEdited = true
  } else if (field === "minPercent") {
    row.minPercent = Math.round(Math.min(row.minHigh, Math.max(row.minLow, Number(value))))
  } else if (field === "rpmInput") {
    row.rpmInput = isNumber(value) ? value : null
  } else if (field === "sensor") {
    row.sensor = String(value)
  } else if (field === "advanced") {
    row.advanced = value === true
  }
  return next
}

// Whether `detected` is the chip `configured` names: the exact name or
// the name followed by "_<suffix>" (daemon/src/config.rs chip_name_matches).
function chipNameMatches(configured, detected) {
  var name = String(configured)
  var found = String(detected)
  if (found === name) return true
  return found.indexOf(name + "_") === 0
}

// The status chip a config alias points to, or null when there is none
// or more than one.
function chipForAlias(config, status, alias) {
  var chips = config && config.chips ? config.chips : {}
  var name = chips[alias]
  if (!name || !status) return null
  var found = listOf(status.chips).filter(function(chip) {
    return chip && chipNameMatches(name, chip.name)
  })
  return found.length === 1 ? found[0] : null
}

// Sensor choices: { value: "cpu/temp1", label: "cpu/temp1 · Tctl · 47 °C" }.
function sensorOptions(config, status) {
  var aliases = config && config.chips ? Object.keys(config.chips).sort() : []
  var options = []
  for (var a = 0; a < aliases.length; a++) {
    var chip = chipForAlias(config, status, aliases[a])
    if (!chip) continue
    var temps = listOf(chip.temps)
    for (var t = 0; t < temps.length; t++) {
      var temp = temps[t]
      if (!temp) continue
      var value = aliases[a] + "/temp" + temp.index
      var parts = [value]
      if (temp.label) parts.push(String(temp.label))
      parts.push(Format.celsius(temp.celsius))
      options.push({ value: value, label: parts.join(" · ") })
    }
  }
  return options
}

// Tachometer choices on a chip: { value: N, label: "fan3 · 815 RPM" }.
function rpmInputOptions(config, status, alias) {
  var chip = chipForAlias(config, status, alias)
  var fans = chip ? listOf(chip.fans) : []
  var options = []
  for (var i = 0; i < fans.length; i++) {
    if (!fans[i]) continue
    options.push({ value: fans[i].index, label: "fan" + fans[i].index + " · " + Format.rpm(fans[i].rpm) })
  }
  return options
}

function optionLabel(options, value) {
  var list = listOf(options)
  for (var i = 0; i < list.length; i++) if (list[i].value === value) return list[i].label
  return value === null || value === undefined ? Format.MISSING : String(value)
}

function noTachometer(row) {
  return !!row && (row.rpm === 0 || row.rpm === null)
}

function controlled(draft) {
  return draft.rows.filter(function(row) { return row.control && !row.pump })
}

// Problems per row key, e.g. { "board/3": "id 'case' is taken" }. Empty
// when every controlled fan can be sent.
function validateDraft(draft) {
  var problems = {}
  var seen = {}
  var rows = pumps(draft).concat(controlled(draft))
  for (var i = 0; i < rows.length; i++) {
    var row = rows[i]
    var problem = null
    if (String(row.label).trim() === "") problem = "give it a name"
    else if (!ID_PATTERN.test(row.id)) problem = "id: lowercase letters, digits and - only"
    else if (seen[row.id]) problem = "id '" + row.id + "' is taken"
    else if (!row.pump && !row.sensor) problem = "pick a sensor"
    else if (!row.pump && !(row.minPercent >= row.minLow && row.minPercent <= row.minHigh)) {
      problem = "minimum must be " + row.minLow + "–" + row.minHigh + "%"
    }
    seen[row.id] = true
    if (problem) problems[row.key] = problem
  }
  return problems
}

function canLeaveStep2(draft) {
  return !!draft && Object.keys(validateDraft(draft)).length === 0
}

// ---------------------------------------------------------------- steps

function canAdvance(draft) {
  if (!draft) return false
  if (draft.step === 1) return canLeaveStep1(draft)
  if (draft.step === 2) return canLeaveStep2(draft)
  return false
}

function next(draft) {
  var copy = clone(draft)
  if (canAdvance(draft)) copy.step = Math.min(STEPS, draft.step + 1)
  return copy
}

function back(draft) {
  var copy = clone(draft)
  copy.step = Math.max(1, draft.step - 1)
  return copy
}

// ---------------------------------------------------------------- step 3

// The daemon's starter curve, raised to the fan's minimum (the daemon does
// the same when a fan comes without a curve).
function starterCurve(starter, minPercent) {
  var floor = isNumber(minPercent) ? minPercent : 0
  return Curve.copy(starter).map(function(point) { return [point[0], Math.max(point[1], floor)] })
}

function curveFor(row, starter) {
  return row.curve && row.curve.length >= 2 ? Curve.copy(row.curve) : starterCurve(starter, row.minPercent)
}

// Cards for step 3.
function summary(draft, starter) {
  return {
    fans: controlled(draft).map(function(row) {
      return {
        key: row.key,
        name: String(row.label).toUpperCase(),
        line: "pwm" + row.pwm + " · " + row.sensor + " · min " + row.minPercent + "%",
        curve: curveFor(row, starter)
      }
    }),
    pumps: pumps(draft).map(function(row) {
      return { key: row.key, name: String(row.label).toUpperCase(), line: outputName(row) }
    })
  }
}

// The `apply_setup` body: { pumps, fans }. Every fan carries its curve,
// so what step 3 shows is what the daemon gets.
function payload(draft, starter) {
  return {
    pumps: pumps(draft).map(function(row) {
      return { id: row.id, label: String(row.label).trim(), chip: row.chip, pwm: row.pwm }
    }),
    fans: controlled(draft).map(function(row) {
      var fan = {
        id: row.id,
        label: String(row.label).trim(),
        chip: row.chip,
        pwm: row.pwm,
        sensor: row.sensor,
        min_percent: Math.round(row.minPercent),
        curve: curveFor(row, starter)
      }
      if (isNumber(row.rpmInput)) fan.rpm_input = row.rpmInput
      return fan
    })
  }
}
