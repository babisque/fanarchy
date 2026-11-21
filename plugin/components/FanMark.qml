pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes

// The Fanarchy mark (design/logo-mark.svg): a ring with a three-blade fan.
// Redrawn with Shapes so its colors come from theme.js. `slashed` swaps the
// fan for a slash through the ring (daemon down).
Item {
  id: root

  property real size: 16
  property color ringColor: "transparent"
  property color fanColor: "transparent"
  property bool slashed: false

  readonly property string bladePath: "M32 34 C29 24 35 17 42 17 C42 25 38 30 32 34 Z"

  implicitWidth: size
  implicitHeight: size

  // Drawn on the SVG's 64 x 64 grid, then scaled to `size`.
  Item {
    width: 64
    height: 64
    scale: root.size / 64
    transformOrigin: Item.TopLeft

    Shape {
      anchors.fill: parent
      preferredRendererType: Shape.CurveRenderer

      ShapePath {
        strokeColor: root.ringColor
        strokeWidth: 6
        fillColor: "transparent"
        PathAngleArc {
          centerX: 32
          centerY: 34
          radiusX: 22
          radiusY: 22
          startAngle: 0
          sweepAngle: 360
        }
      }

      ShapePath {
        strokeColor: root.slashed ? root.ringColor : "transparent"
        strokeWidth: 6
        fillColor: "transparent"
        capStyle: ShapePath.FlatCap
        PathSvg { path: "M13 53 L51 15" }
      }
    }

    Repeater {
      model: root.slashed ? [] : [0, 120, 240]

      Shape {
        id: blade
        required property int modelData
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer
        transform: Rotation { origin.x: 32; origin.y: 34; angle: blade.modelData }

        ShapePath {
          strokeColor: "transparent"
          fillColor: root.fanColor
          PathSvg { path: root.bladePath }
        }
      }
    }
  }
}
