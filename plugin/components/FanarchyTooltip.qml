pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Quickshell
import "../models/theme.js" as Theme
import "../models/placement.js" as Placement

// Hover card for the bar widget: a paper note rotated -1° with a strip of
// tape across the top. It never takes input, so it can't steal the hover
// from the button underneath.
PopupWindow {
  id: root

  required property Item anchorItem
  property var bar: null
  property bool open: false
  property var view: ({ state: "down", rows: [], downReason: "stopped" })
  property string stencilFamily: Theme.fonts.stencil.family
  property string monoFamily: Theme.fonts.mono.family
  property string noteFamily: Theme.fonts.note.family

  // Room around the card for its rotation and the tape that sticks out.
  readonly property int bleed: 12
  readonly property int tapeOverhang: Math.round(Theme.tooltip.tapeHeight / 2)
  readonly property bool down: view.state === "down"

  visible: open || frame.opacity > 0
  color: "transparent"
  mask: Region {}
  implicitWidth: Theme.tooltip.width + bleed * 2
  implicitHeight: card.height + tapeOverhang + bleed * 2

  anchor {
    window: root.anchorItem ? root.anchorItem.QsWindow.window : null
    adjustment: PopupAdjustment.Slide
    edges: Edges.Top | Edges.Left
    gravity: Edges.Bottom | Edges.Right
    rect.width: 1
    rect.height: 1

    onAnchoring: {
      var target = root.anchorItem
      var window = target ? target.QsWindow.window : null
      if (!target || !window) return
      var position = root.bar ? root.bar.position : "top"
      var popup = { width: root.implicitWidth, height: root.implicitHeight }
      var local = Placement.tooltipOffset(position, target, popup, Theme.tooltip.offset - root.bleed)
      var mapped = window.contentItem.mapFromItem(target, local.x, local.y)
      var point = Placement.clampToWindow(position, mapped, popup, window, 0)
      root.anchor.rect.x = point.x
      root.anchor.rect.y = point.y
    }
  }

  Item {
    id: frame
    anchors.fill: parent
    opacity: root.open ? 1 : 0

    Behavior on opacity {
      NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad }
    }

    Rectangle {
      id: card
      x: root.bleed
      y: root.bleed + root.tapeOverhang
      width: Theme.tooltip.width
      height: content.implicitHeight + Theme.tooltip.padding * 2
      color: Theme.colors.paper
      rotation: Theme.tooltip.rotation
      antialiasing: true

      ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: Theme.tooltip.padding
        spacing: 10

        Text {
          text: "FANARCHY"
          color: Theme.colors.ink
          font.family: root.stencilFamily
          font.weight: Theme.fonts.stencil.weight
          font.pixelSize: Theme.tooltip.title
          font.letterSpacing: 1
        }

        // Live readings: one name / value line per row.
        ColumnLayout {
          visible: !root.down
          Layout.fillWidth: true
          spacing: Theme.tooltip.rowGap

          Repeater {
            model: root.view.rows

            RowLayout {
              id: entry
              required property var modelData
              Layout.fillWidth: true
              spacing: Theme.tooltip.columnGap

              Text {
                Layout.fillWidth: true
                text: entry.modelData.name
                elide: Text.ElideRight
                color: Theme.colors.ink
                font.family: root.monoFamily
                font.pixelSize: Theme.tooltip.value
              }

              Text {
                text: entry.modelData.value
                color: Theme.colors.ink
                font.family: root.monoFamily
                font.weight: Theme.fonts.mono.weightBold
                font.pixelSize: Theme.tooltip.value
              }
            }
          }
        }

        // Daemon unreachable: say why and how to fix it.
        ColumnLayout {
          visible: root.down
          Layout.fillWidth: true
          spacing: 8

          Text {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: root.view.downReason === "permission"
              ? "No access to /run/fanarchy.sock. Join the fanarchy group, then log in again."
              : "Daemon stopped. Fans are on BIOS control."
            color: Theme.colors.ink
            font.family: root.monoFamily
            font.pixelSize: Theme.tooltip.value
          }

          Rectangle {
            Layout.fillWidth: true
            implicitHeight: command.implicitHeight + 12
            color: Theme.colors.inkRaised

            Text {
              id: command
              anchors.verticalCenter: parent.verticalCenter
              anchors.left: parent.left
              anchors.leftMargin: 8
              text: root.view.downReason === "permission"
                ? "sudo usermod -aG fanarchy $USER"
                : "sudo systemctl start fanarchyd"
              color: Theme.colors.paper
              font.family: root.monoFamily
              font.pixelSize: Theme.tooltip.value
            }
          }
        }

        Text {
          text: "click to open the panel"
          color: Theme.colors.ink
          font.family: root.noteFamily
          font.pixelSize: Theme.tooltip.note
        }
      }

      Image {
        source: Qt.resolvedUrl("../assets/tape-2.png")
        width: Theme.tooltip.tapeWidth
        height: Theme.tooltip.tapeHeight
        fillMode: Image.Stretch
        anchors.horizontalCenter: parent.horizontalCenter
        y: -root.tapeOverhang
        rotation: Theme.tooltip.tapeRotation
        smooth: true
        mipmap: true
      }
    }
  }
}
