import QtQuick
import QtQuick.Layouts
import "../models/theme.js" as Theme

// Daemon stopped / no fans configured. Copy comes from panel.js
// emptyState(): { headline, line, command, button }.
Item {
  id: root

  required property var copy
  property var fonts: null
  property bool done: false      // shows "✓ COPIED" on the button for a moment

  signal action()

  implicitHeight: column.implicitHeight + Theme.tooltip.tapeHeight

  function flashDone() {
    root.done = true
    doneTimer.restart()
  }

  Timer {
    id: doneTimer
    interval: Theme.empty.copiedMs
    onTriggered: root.done = false
  }

  Image {
    source: Qt.resolvedUrl("../assets/tape-1.png")
    width: Theme.tooltip.tapeWidth
    height: Theme.tooltip.tapeHeight
    anchors.horizontalCenter: parent.horizontalCenter
    y: 0
    rotation: -Theme.tooltip.tapeRotation
    fillMode: Image.Stretch
    smooth: true
    mipmap: true
  }

  ColumnLayout {
    id: column
    y: Theme.tooltip.tapeHeight
    width: parent.width
    spacing: 16

    Image {
      Layout.alignment: Qt.AlignHCenter
      source: Qt.resolvedUrl("../assets/logo.svg")
      sourceSize.width: Theme.empty.logo
      sourceSize.height: Theme.empty.logo
      Layout.preferredWidth: Theme.empty.logo
      Layout.preferredHeight: Theme.empty.logo
      opacity: Theme.empty.logoOpacity
    }

    Text {
      Layout.fillWidth: true
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.WordWrap
      text: root.copy.headline
      color: Theme.colors.paper
      font.family: root.fonts ? root.fonts.stencil : Theme.fonts.stencil.family
      font.weight: Theme.fonts.stencil.weight
      font.pixelSize: Theme.empty.headline
      lineHeight: 0.95
    }

    Text {
      Layout.fillWidth: true
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.WordWrap
      text: root.copy.line
      color: Theme.colors.ash
      font.family: root.fonts ? root.fonts.note : Theme.fonts.note.family
      font.pixelSize: Theme.empty.line
    }

    // The command in a dashed code box (none for START SETUP).
    Item {
      Layout.fillWidth: true
      visible: root.copy.command !== ""
      implicitHeight: code.implicitHeight + 24

      Rectangle {
        anchors.fill: parent
        color: Theme.colors.inkRaised
      }

      DashedBox {
        anchors.fill: parent
        strokeColor: Theme.colors.ash
      }

      Text {
        id: code
        anchors.centerIn: parent
        text: root.copy.command
        textFormat: Text.PlainText
        color: Theme.colors.paper
        font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
        font.pixelSize: Theme.empty.code
      }
    }

    Rectangle {
      id: button
      Layout.alignment: Qt.AlignHCenter
      Layout.preferredWidth: Math.max(200, buttonLabel.implicitWidth + 48)
      Layout.preferredHeight: Theme.empty.button
      color: buttonMouse.containsMouse ? Theme.colors.paper : Theme.colors.riot
      scale: buttonMouse.pressed ? Theme.bar.pressScale : 1

      Accessible.role: Accessible.Button
      Accessible.name: root.copy.command !== "" ? root.copy.button + ": " + root.copy.command : root.copy.button
      Accessible.onPressAction: root.action()

      Behavior on color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
      Behavior on scale { NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }

      Text {
        id: buttonLabel
        anchors.centerIn: parent
        text: root.done ? "✓ COPIED" : root.copy.button
        color: Theme.colors.ink
        font.family: root.fonts ? root.fonts.stencil : Theme.fonts.stencil.family
        font.weight: Theme.fonts.stencil.weight
        font.pixelSize: Theme.modes.text
        font.letterSpacing: 1
      }

      MouseArea {
        id: buttonMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.action()
      }
    }
  }
}
