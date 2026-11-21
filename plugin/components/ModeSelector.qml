pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import "../models/theme.js" as Theme
import "../models/panel.js" as Panel

// CURVE / FIXED / BIOS for the selected fan, plus the FIXED speed slider.
// The slider value is local while dragging and is sent only on release.
ColumnLayout {
  id: root

  property var fonts: null
  property string mode: ""          // "curve", "fixed", "auto" or "" (unknown)
  property real percent: 50         // starting value for the slider
  property real minPercent: 0
  property bool interactive: true

  signal modeRequested(string mode)
  signal fixedRequested(int percent)

  property real sliderValue: percent
  property bool sliding: false

  onPercentChanged: if (!sliding) sliderValue = percent

  spacing: 12

  RowLayout {
    Layout.fillWidth: true
    spacing: Theme.modes.gap

    Repeater {
      model: Panel.MODES

      Rectangle {
        id: modeButton
        required property var modelData
        readonly property bool selected: root.mode === modelData.mode

        Layout.fillWidth: true
        Layout.preferredWidth: 1
        Layout.preferredHeight: Theme.modes.height
        color: selected ? Theme.colors.riot : Theme.colors.transparent
        border.width: selected ? 0 : Theme.modes.border
        border.color: modeMouse.containsMouse && root.interactive ? Theme.colors.ash : Theme.colors.line
        scale: modeMouse.pressed ? Theme.bar.pressScale : 1
        opacity: root.interactive ? 1 : 0.4

        Accessible.role: Accessible.RadioButton
        Accessible.name: "Mode " + modelData.label
        Accessible.checked: selected
        Accessible.onPressAction: if (root.interactive) root.modeRequested(modelData.mode)

        Behavior on color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
        Behavior on border.color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
        Behavior on scale { NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }

        Text {
          anchors.centerIn: parent
          text: modeButton.modelData.label
          color: modeButton.selected ? Theme.colors.ink : Theme.colors.paper
          font.family: root.fonts ? root.fonts.stencil : Theme.fonts.stencil.family
          font.weight: Theme.fonts.stencil.weight
          font.pixelSize: Theme.modes.text
          font.letterSpacing: 1

          Behavior on color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
        }

        MouseArea {
          id: modeMouse
          anchors.fill: parent
          enabled: root.interactive
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: if (!modeButton.selected) root.modeRequested(modeButton.modelData.mode)
        }
      }
    }
  }

  // FIXED speed, from the fan's minimum to 100 %.
  RowLayout {
    Layout.fillWidth: true
    visible: root.mode === "fixed"
    spacing: 14

    Item {
      id: slider
      Layout.fillWidth: true
      Layout.preferredHeight: Theme.modes.sliderHeight

      readonly property real span: Math.max(1, 100 - root.minPercent)
      readonly property real ratio: (root.sliderValue - root.minPercent) / span

      Accessible.role: Accessible.Slider
      Accessible.name: "Fixed speed " + Math.round(root.sliderValue) + " percent"

      function valueAt(x) {
        var r = Math.min(1, Math.max(0, x / width))
        return Math.round(root.minPercent + r * span)
      }

      Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: parent.width
        height: Theme.modes.track
        color: Theme.colors.line
      }

      Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: slider.ratio * parent.width
        height: Theme.modes.track
        color: Theme.colors.paper
      }

      Rectangle {
        width: Theme.modes.handle
        height: Theme.modes.handle
        anchors.verticalCenter: parent.verticalCenter
        x: slider.ratio * parent.width - width / 2
        color: root.sliding ? Theme.colors.paper : Theme.colors.ink
        border.width: Theme.editor.pointStroke
        border.color: root.sliding ? Theme.colors.riot : Theme.colors.paper

        Behavior on color { ColorAnimation { duration: Theme.motion.duration } }
      }

      MouseArea {
        anchors.fill: parent
        anchors.margins: -6
        enabled: root.interactive
        cursorShape: Qt.PointingHandCursor
        preventStealing: true
        onPressed: function(mouse) {
          root.sliding = true
          root.sliderValue = slider.valueAt(mouse.x - 6)
        }
        onPositionChanged: function(mouse) {
          if (root.sliding) root.sliderValue = slider.valueAt(mouse.x - 6)
        }
        onReleased: {
          root.sliding = false
          root.fixedRequested(Math.round(root.sliderValue))
        }
        onCanceled: root.sliding = false
      }
    }

    Text {
      Layout.preferredWidth: 52
      horizontalAlignment: Text.AlignRight
      text: Math.round(root.sliderValue) + "%"
      color: Theme.colors.paper
      font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
      font.weight: Theme.fonts.mono.weightBold
      font.pixelSize: Theme.panel.temps
    }
  }
}
