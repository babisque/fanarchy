.pragma library

// Where the tooltip goes relative to the bar button, for each bar position.
// Returns the popup's top-left corner in the button's own coordinates.
function tooltipOffset(barPosition, target, popup, margin) {
  var x = target.width / 2 - popup.width / 2
  var y = target.height + margin
  if (barPosition === "bottom") {
    y = -popup.height - margin
  } else if (barPosition === "left") {
    x = target.width + margin
    y = target.height / 2 - popup.height / 2
  } else if (barPosition === "right") {
    x = -popup.width - margin
    y = target.height / 2 - popup.height / 2
  }
  return { x: x, y: y }
}

// Keep the popup inside the window along the bar's axis.
function clampToWindow(barPosition, point, popup, windowSize, margin) {
  var horizontal = barPosition !== "left" && barPosition !== "right"
  if (horizontal) {
    var maxX = Math.max(margin, windowSize.width - popup.width - margin)
    return { x: Math.round(Math.min(Math.max(point.x, margin), maxX)), y: Math.round(point.y) }
  }
  var maxY = Math.max(margin, windowSize.height - popup.height - margin)
  return { x: Math.round(point.x), y: Math.round(Math.min(Math.max(point.y, margin), maxY)) }
}
