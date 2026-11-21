import QtQuick
import QtQuick.Shapes
import "../models/theme.js" as Theme

// A dashed rectangle outline (Rectangle has no dashed borders).
Shape {
  id: root

  property color strokeColor: Theme.colors.ash
  property real strokeWidth: 1
  property real dash: 3

  preferredRendererType: Shape.CurveRenderer

  ShapePath {
    strokeColor: root.strokeColor
    strokeWidth: root.strokeWidth
    strokeStyle: ShapePath.DashLine
    dashPattern: [root.dash, root.dash]
    fillColor: "transparent"
    PathRectangle {
      x: root.strokeWidth / 2
      y: root.strokeWidth / 2
      width: Math.max(0, root.width - root.strokeWidth)
      height: Math.max(0, root.height - root.strokeWidth)
    }
  }
}
