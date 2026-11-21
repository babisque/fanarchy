.pragma library

// Fan curve rules for the editor. Pure functions only: every function
// returns a new array and never changes its input.
//
// A point is [temp_c, percent], the same shape as the config and the
// set_curve request. The daemon validates every curve again (docs/protocol.md):
// at least 2 points, temperatures 0–120 °C strictly increasing, percentages
// 0–100 never decreasing.

var CHART_MIN_C = 20
var CHART_MAX_C = 100
var MIN_POINTS = 2
var MAX_POINTS = 8

function clamp(value, low, high) {
  return Math.min(Math.max(value, low), high)
}

function copy(points) {
  var out = []
  var list = Array.isArray(points) ? points : []
  for (var i = 0; i < list.length; i++) out.push([Number(list[i][0]), Number(list[i][1])])
  return out
}

function sorted(points) {
  return copy(points).sort(function(a, b) { return a[0] - b[0] })
}

function equal(a, b) {
  if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false
  for (var i = 0; i < a.length; i++) {
    if (Number(a[i][0]) !== Number(b[i][0]) || Number(a[i][1]) !== Number(b[i][1])) return false
  }
  return true
}

function floorOf(minPercent) {
  return typeof minPercent === "number" && isFinite(minPercent) ? clamp(minPercent, 0, 100) : 0
}

// Speed (%) the daemon applies at `tempC`: linear between points, flat
// before the first and after the last (daemon/src/curve.rs `evaluate`).
function evaluate(points, tempC) {
  var list = sorted(points)
  if (list.length === 0 || typeof tempC !== "number" || !isFinite(tempC)) return 100
  if (tempC <= list[0][0]) return list[0][1]
  var last = list[list.length - 1]
  if (tempC >= last[0]) return last[1]
  for (var i = 1; i < list.length; i++) {
    var t0 = list[i - 1][0], p0 = list[i - 1][1]
    var t1 = list[i][0], p1 = list[i][1]
    if (tempC <= t1) return t1 - t0 <= 0 ? p1 : p0 + (p1 - p0) * (tempC - t0) / (t1 - t0)
  }
  return last[1]
}

// Move point `index` to (tempC, percent), kept between its neighbours so the
// order never changes while dragging: at least 1 °C from each neighbour,
// percent between the previous point (or the fan's minimum) and the next.
function movePoint(points, index, tempC, percent, minPercent) {
  var list = sorted(points)
  if (index < 0 || index >= list.length) return list
  var floor = floorOf(minPercent)
  var prev = index > 0 ? list[index - 1] : null
  var next = index < list.length - 1 ? list[index + 1] : null

  var lowT = prev ? prev[0] + 1 : CHART_MIN_C
  var highT = next ? next[0] - 1 : CHART_MAX_C
  var lowP = prev ? Math.max(prev[1], floor) : floor
  var highP = next ? next[1] : 100

  var t = Math.round(clamp(Number(tempC), lowT, Math.max(lowT, highT)))
  var p = Math.round(clamp(Number(percent), lowP, Math.max(lowP, highP)))
  list[index] = [t, p]
  return list
}

// Add a point at `tempC`, on the current curve (and never under the fan's
// minimum). Returns the same curve when full or when that temperature is
// already taken.
function addPointAt(points, tempC, minPercent) {
  var list = sorted(points)
  if (list.length >= MAX_POINTS) return list
  var t = Math.round(clamp(Number(tempC), CHART_MIN_C, CHART_MAX_C))
  for (var i = 0; i < list.length; i++) if (list[i][0] === t) return list
  var p = Math.round(Math.max(evaluate(list, t), floorOf(minPercent)))
  list.push([t, Math.min(p, 100)])
  return sorted(list)
}

// Add a point in the middle of the widest gap between two points.
function addPoint(points, minPercent) {
  var list = sorted(points)
  if (list.length < 2) return addPointAt(list, (CHART_MIN_C + CHART_MAX_C) / 2, minPercent)
  var best = -1, width = 1
  for (var i = 1; i < list.length; i++) {
    var gap = list[i][0] - list[i - 1][0]
    if (gap > width) { width = gap; best = i }
  }
  if (best === -1) return list
  return addPointAt(list, Math.round((list[best - 1][0] + list[best][0]) / 2), minPercent)
}

function removePoint(points, index) {
  var list = sorted(points)
  if (list.length <= MIN_POINTS || index < 0 || index >= list.length) return list
  list.splice(index, 1)
  return list
}

function canAdd(points) {
  return Array.isArray(points) && points.length < MAX_POINTS
}

function canRemove(points) {
  return Array.isArray(points) && points.length > MIN_POINTS
}

// Null when the curve is valid, otherwise what is wrong with it.
function validate(points, minPercent) {
  if (!Array.isArray(points) || points.length < MIN_POINTS) return "a curve needs at least 2 points"
  if (points.length > MAX_POINTS) return "a curve has at most 8 points"
  var floor = floorOf(minPercent)
  for (var i = 0; i < points.length; i++) {
    var t = points[i][0], p = points[i][1]
    if (typeof t !== "number" || !isFinite(t) || t < 0 || t > 120) return "temperatures go from 0 to 120 °C"
    if (typeof p !== "number" || !isFinite(p) || p < 0 || p > 100) return "speeds go from 0 to 100 %"
    if (p < floor) return "the curve drops below this fan's minimum (" + floor + "%)"
    if (i > 0 && t <= points[i - 1][0]) return "temperatures must keep rising"
    if (i > 0 && p < points[i - 1][1]) return "speeds must never go down as it heats up"
  }
  return null
}

// Chart mapping: X is CHART_MIN_C–CHART_MAX_C, Y is 0–100 % (0 at the bottom).
function xFor(tempC, width) {
  return (clamp(tempC, CHART_MIN_C, CHART_MAX_C) - CHART_MIN_C) / (CHART_MAX_C - CHART_MIN_C) * width
}

function yFor(percent, height) {
  return height - clamp(percent, 0, 100) / 100 * height
}

function tempAt(x, width) {
  if (!(width > 0)) return CHART_MIN_C
  return CHART_MIN_C + clamp(x / width, 0, 1) * (CHART_MAX_C - CHART_MIN_C)
}

function percentAt(y, height) {
  if (!(height > 0)) return 0
  return (1 - clamp(y / height, 0, 1)) * 100
}

// Polyline through the curve across the whole chart, flat at both ends.
function polyline(points, width, height) {
  var list = sorted(points)
  if (list.length === 0) return []
  var out = [{ x: 0, y: yFor(list[0][1], height) }]
  for (var i = 0; i < list.length; i++) out.push({ x: xFor(list[i][0], width), y: yFor(list[i][1], height) })
  out.push({ x: width, y: yFor(list[list.length - 1][1], height) })
  return out
}

function pointLabel(point) {
  return Math.round(point[0]) + "° → " + Math.round(point[1]) + "%"
}

// Gridline temperatures and percents for the axes.
function tempTicks() {
  var out = []
  for (var t = CHART_MIN_C; t <= CHART_MAX_C; t += 10) out.push(t)
  return out
}

function percentTicks() {
  return [0, 25, 50, 75, 100]
}
