pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import "../models/theme.js" as Theme
import "../models/curve.js" as Curve

// Fan curve chart: X 20–100 °C, Y 0–100 %. Points are dragged on a local
// copy; `committed` fires only on release (and on add / remove), never on
// every dragged pixel.
Item {
  id: root

  property var fonts: null
  property string title: ""
  property string sensor: ""
  property var points: []            // curve the daemon is running
  property real minPercent: 0
  property var currentTemp: null     // °C from the fan's sensor, or null
  property real criticalTemp: 85
  property bool interactive: true

  signal committed(var points)

  property var dragPoints: null      // live copy while a point is held
  property int dragIndex: -1
  property int selectedIndex: -1

  readonly property var shown: dragPoints !== null ? dragPoints : Curve.sorted(points)
  readonly property bool hasTemp: typeof currentTemp === "number" && isFinite(currentTemp)
  readonly property string monoFamily: fonts ? fonts.mono : Theme.fonts.mono.family

  implicitHeight: column.implicitHeight

  onPointsChanged: if (selectedIndex >= points.length) selectedIndex = -1

  function commit(next) {
    if (!Curve.equal(next, Curve.sorted(root.points))) root.committed(next)
  }

  ColumnLayout {
    id: column
    width: parent.width
    spacing: 12

    RowLayout {
      Layout.fillWidth: true

      Text {
        Layout.fillWidth: true
        text: root.title
        elide: Text.ElideRight
        color: Theme.colors.paper
        font.family: root.fonts ? root.fonts.stencil : Theme.fonts.stencil.family
        font.weight: Theme.fonts.stencil.weight
        font.pixelSize: Theme.editor.title
      }

      Text {
        text: root.sensor
        color: Theme.colors.ash
        font.family: root.monoFamily
        font.pixelSize: Theme.editor.sensor
      }
    }

    // Chart with its axis labels.
    Item {
      id: frame
      Layout.fillWidth: true
      Layout.preferredHeight: Theme.editor.chartHeight + Theme.editor.axisFooter

      // % labels on the left.
      Repeater {
        model: Curve.percentTicks()

        Text {
          required property int modelData
          width: Theme.editor.axisGutter - 6
          horizontalAlignment: Text.AlignRight
          y: 1 + Curve.yFor(modelData, plot.height) - height / 2
          text: modelData + "%"
          color: Theme.colors.ash
          font.family: root.monoFamily
          font.pixelSize: Theme.editor.axis
        }
      }

      // °C labels below.
      Repeater {
        model: Curve.tempTicks()

        Text {
          required property int modelData
          x: chart.x + 1 + Curve.xFor(modelData, plot.width) - width / 2
          y: Theme.editor.chartHeight + 4
          text: modelData + "°"
          color: Theme.colors.ash
          font.family: root.monoFamily
          font.pixelSize: Theme.editor.axis
        }
      }

      Rectangle {
        id: chart
        x: Theme.editor.axisGutter
        width: parent.width - Theme.editor.axisGutter
        height: Theme.editor.chartHeight
        color: Theme.colors.inkRaised
        border.width: 1
        border.color: Theme.colors.line

        Item {
          id: plot
          anchors.fill: parent
          anchors.margins: 1

          // Double-click empty chart space to add a point there.
          MouseArea {
            anchors.fill: parent
            enabled: root.interactive
            acceptedButtons: Qt.LeftButton
            onDoubleClicked: function(mouse) {
              if (!Curve.canAdd(root.points)) return
              root.commit(Curve.addPointAt(root.points, Curve.tempAt(mouse.x, plot.width), root.minPercent))
            }
          }

          // Grid.
          Repeater {
            model: Curve.tempTicks()
            Rectangle {
              required property int modelData
              x: Math.round(Curve.xFor(modelData, plot.width))
              width: 1
              height: plot.height
              color: Theme.colors.line
            }
          }

          Repeater {
            model: Curve.percentTicks()
            Rectangle {
              required property int modelData
              y: Math.round(Curve.yFor(modelData, plot.height))
              width: plot.width
              height: 1
              color: Theme.colors.line
            }
          }

          // Danger zone: everything at or above the critical temperature.
          Item {
            id: danger
            x: Curve.xFor(root.criticalTemp, plot.width)
            width: Math.max(0, plot.width - x)
            height: plot.height
            visible: width > 0
            clip: true

            Canvas {
              id: hatch
              anchors.fill: parent
              opacity: Theme.editor.hatchOpacity
              onWidthChanged: requestPaint()
              onHeightChanged: requestPaint()
              onPaint: {
                var ctx = getContext("2d")
                ctx.reset()
                ctx.strokeStyle = Theme.colors.riot
                ctx.lineWidth = 2
                var gap = Theme.editor.hatchGap
                for (var d = -height; d < width; d += gap) {
                  ctx.beginPath()
                  ctx.moveTo(d, height)
                  ctx.lineTo(d + height, 0)
                  ctx.stroke()
                }
              }
            }

            Rectangle {
              x: 4
              y: 4
              width: dangerLabel.implicitWidth + 8
              height: dangerLabel.implicitHeight + 4
              color: Theme.colors.ink

              Text {
                id: dangerLabel
                anchors.centerIn: parent
                text: "≥" + Math.round(root.criticalTemp) + "° FULL"
                color: Theme.colors.paper
                font.family: root.monoFamily
                font.weight: Theme.fonts.mono.weightBold
                font.pixelSize: Theme.editor.label
              }
            }
          }

          // The fan's minimum: the curve can't go under this line.
          Shape {
            anchors.fill: parent
            visible: root.minPercent > 0
            opacity: Theme.editor.minLineOpacity
            ShapePath {
              strokeColor: Theme.colors.ash
              strokeWidth: 1
              strokeStyle: ShapePath.DashLine
              dashPattern: [Theme.editor.dash, Theme.editor.dash]
              fillColor: "transparent"
              startX: 0
              startY: Curve.yFor(root.minPercent, plot.height)
              PathLine { x: plot.width; y: Curve.yFor(root.minPercent, plot.height) }
            }
          }

          Text {
            visible: root.minPercent > 0
            x: 6
            y: Curve.yFor(root.minPercent, plot.height) + 2
            text: "min " + Math.round(root.minPercent) + "%"
            color: Theme.colors.ash
            font.family: root.monoFamily
            font.pixelSize: Theme.editor.axis
          }

          // The curve, flat before the first point and after the last.
          Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
              strokeColor: Theme.colors.paper
              strokeWidth: Theme.editor.curveWidth
              fillColor: "transparent"
              joinStyle: ShapePath.RoundJoin
              capStyle: ShapePath.FlatCap
              PathPolyline {
                path: Curve.polyline(root.shown, plot.width, plot.height).map(function(p) {
                  return Qt.point(p.x, p.y)
                })
              }
            }
          }

          // Right now: dashed riot line at the sensor's temperature, with a
          // dot where it meets the curve.
          Shape {
            anchors.fill: parent
            visible: root.hasTemp
            ShapePath {
              strokeColor: Theme.colors.riot
              strokeWidth: Theme.editor.markerWidth
              strokeStyle: ShapePath.DashLine
              dashPattern: [Theme.editor.dash / 2, Theme.editor.dash / 2]
              fillColor: "transparent"
              startX: root.hasTemp ? Curve.xFor(root.currentTemp, plot.width) : 0
              startY: 0
              PathLine {
                x: root.hasTemp ? Curve.xFor(root.currentTemp, plot.width) : 0
                y: plot.height
              }
            }
          }

          Rectangle {
            visible: root.hasTemp && root.shown.length > 0
            width: Theme.editor.markerDot
            height: width
            radius: width / 2
            color: Theme.colors.riot
            x: (root.hasTemp ? Curve.xFor(root.currentTemp, plot.width) : 0) - width / 2
            y: Curve.yFor(Curve.evaluate(root.shown, root.hasTemp ? root.currentTemp : 0), plot.height) - height / 2
          }

          // Points. The model is a count, so dragging (which replaces the
          // array every move) never recreates the delegate under the mouse.
          Repeater {
            model: root.shown.length

            Item {
              id: handle
              required property int index
              readonly property var point: root.shown[index] || [0, 0]
              readonly property bool held: root.dragIndex === index

              x: Curve.xFor(point[0], plot.width) - width / 2
              y: Curve.yFor(point[1], plot.height) - height / 2
              width: Theme.editor.hitArea
              height: Theme.editor.hitArea
              z: held ? 2 : 1

              Accessible.role: Accessible.Slider
              Accessible.name: "Curve point " + (index + 1) + ": " + Curve.pointLabel(point)

              Rectangle {
                anchors.centerIn: parent
                width: handle.held ? Theme.editor.pointDragged : Theme.editor.point
                height: width
                radius: width / 2
                color: handle.held ? Theme.colors.paper : Theme.colors.ink
                border.width: Theme.editor.pointStroke
                border.color: handle.held || root.selectedIndex === handle.index
                  ? Theme.colors.riot : Theme.colors.paper
                antialiasing: true

                Behavior on width { NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
                Behavior on color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
              }

              MouseArea {
                anchors.fill: parent
                enabled: root.interactive
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                cursorShape: handle.held ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                preventStealing: true

                onPressed: function(mouse) {
                  root.selectedIndex = handle.index
                  if (mouse.button === Qt.RightButton) return
                  root.dragPoints = Curve.sorted(root.points)
                  root.dragIndex = handle.index
                }

                onPositionChanged: function(mouse) {
                  if (root.dragIndex !== handle.index || root.dragPoints === null) return
                  var p = mapToItem(plot, mouse.x, mouse.y)
                  root.dragPoints = Curve.movePoint(root.dragPoints, handle.index,
                    Curve.tempAt(p.x, plot.width), Curve.percentAt(p.y, plot.height), root.minPercent)
                }

                onReleased: function(mouse) {
                  if (mouse.button === Qt.RightButton) {
                    if (Curve.canRemove(root.points)) {
                      root.selectedIndex = -1
                      root.commit(Curve.removePoint(root.points, handle.index))
                    }
                    return
                  }
                  var final = root.dragPoints
                  root.dragIndex = -1
                  root.dragPoints = null
                  if (final !== null) root.commit(final)
                }

                onCanceled: {
                  root.dragIndex = -1
                  root.dragPoints = null
                }
              }
            }
          }

          // "65° → 55%" next to the held point.
          Rectangle {
            id: dragLabel
            readonly property var point: root.dragIndex >= 0 && root.shown[root.dragIndex] ? root.shown[root.dragIndex] : null
            visible: point !== null
            z: 3
            width: dragText.implicitWidth + 10
            height: dragText.implicitHeight + 6
            color: Theme.colors.paper
            x: point ? Math.min(plot.width - width, Math.max(0, Curve.xFor(point[0], plot.width) - width / 2)) : 0
            y: point ? Math.max(0, Curve.yFor(point[1], plot.height) - height - Theme.editor.hitArea / 2) : 0

            Text {
              id: dragText
              anchors.centerIn: parent
              text: dragLabel.point ? Curve.pointLabel(dragLabel.point) : ""
              color: Theme.colors.ink
              font.family: root.monoFamily
              font.weight: Theme.fonts.mono.weightBold
              font.pixelSize: Theme.editor.label
            }
          }
        }
      }
    }

    RowLayout {
      Layout.fillWidth: true
      spacing: 8

      Text {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: "Drag a point. Release to apply. Red line = right now."
        color: Theme.colors.ash
        font.family: root.fonts ? root.fonts.note : Theme.fonts.note.family
        font.pixelSize: Theme.editor.note
      }

      Repeater {
        model: [
          { label: "+ POINT", name: "Add a point", add: true },
          { label: "− POINT", name: "Remove the selected point", add: false }
        ]

        Rectangle {
          id: pointButton
          required property var modelData
          readonly property bool usable: root.interactive && (modelData.add
            ? Curve.canAdd(root.points)
            : Curve.canRemove(root.points) && root.selectedIndex >= 0)

          implicitWidth: pointLabel.implicitWidth + 20
          implicitHeight: Theme.editor.button
          color: Theme.colors.transparent
          border.width: Theme.modes.border
          border.color: pointMouse.containsMouse && usable ? Theme.colors.ash : Theme.colors.line
          opacity: usable ? 1 : 0.4

          Accessible.role: Accessible.Button
          Accessible.name: modelData.name
          Accessible.onPressAction: if (usable) activate()

          function activate() {
            if (modelData.add) {
              root.commit(Curve.addPoint(root.points, root.minPercent))
            } else {
              var index = root.selectedIndex
              root.selectedIndex = -1
              root.commit(Curve.removePoint(root.points, index))
            }
          }

          Behavior on border.color { ColorAnimation { duration: Theme.motion.duration } }
          Behavior on opacity { NumberAnimation { duration: Theme.motion.duration } }

          Text {
            id: pointLabel
            anchors.centerIn: parent
            text: pointButton.modelData.label
            color: Theme.colors.paper
            font.family: root.monoFamily
            font.weight: Theme.fonts.mono.weightBold
            font.pixelSize: Theme.editor.label
          }

          MouseArea {
            id: pointMouse
            anchors.fill: parent
            enabled: pointButton.usable
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: pointButton.activate()
          }
        }
      }
    }
  }
}
