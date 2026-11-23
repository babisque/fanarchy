pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import "../models/theme.js" as Theme
import "../models/format.js" as Format
import "../models/setup.js" as Setup

// One PWM output in the setup wizard: "pwm2 · board", its path and live
// RPM, plus a toggle. Step 1 ("pump"): the PUMP toggle. Step 2 ("fan"):
// the CONTROL toggle and, when on, the fan's settings. "readonly": a pump
// shown on step 2. A marked pump gets the dashed frame and "HANDS OFF"
// tape of PumpCard. Decisions live in setup.js; this only emits changes.
Item {
  id: root

  required property var row            // a setup.js draft row
  property string kind: "pump"         // "pump", "fan" or "readonly"
  property var fonts: null
  property string problem: ""          // setup.js validateDraft() message
  property var sensorOptions: []
  property var rpmOptions: []

  signal toggled()
  signal fieldChanged(string field, var value)

  readonly property bool isPump: row.pump
  readonly property bool expanded: kind === "fan" && row.control && !row.pump
  readonly property string stencil: fonts ? fonts.stencil : Theme.fonts.stencil.family
  readonly property string mono: fonts ? fonts.mono : Theme.fonts.mono.family
  readonly property color nameColor: isPump ? Theme.colors.ash : Theme.colors.paper

  implicitHeight: column.implicitHeight + Theme.card.padY * 2

  // Frame: dashed ash for pumps, a plain line otherwise.
  Rectangle {
    anchors.fill: parent
    visible: !root.isPump
    color: Theme.colors.transparent
    border.width: Theme.card.border
    border.color: root.expanded ? Theme.colors.ash : Theme.colors.line
  }

  DashedBox {
    anchors.fill: parent
    visible: root.isPump
    strokeColor: Theme.colors.ash
  }

  ColumnLayout {
    id: column
    x: Theme.card.padX
    y: Theme.card.padY
    width: parent.width - Theme.card.padX * 2
    spacing: Theme.wizard.gap

    RowLayout {
      Layout.fillWidth: true
      spacing: Theme.card.padX

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2

        Text {
          Layout.fillWidth: true
          elide: Text.ElideRight
          text: Setup.outputName(root.row)
                + (root.isPump || root.row.control ? " · " + root.row.label : "")
          color: root.nameColor
          font.family: root.stencil
          font.weight: Theme.fonts.stencil.weight
          font.pixelSize: Theme.wizard.rowName
        }

        Text {
          Layout.fillWidth: true
          elide: Text.ElideMiddle
          text: root.row.path
          color: Theme.colors.ash
          font.family: root.mono
          font.pixelSize: Theme.wizard.rowMeta
        }
      }

      Text {
        text: Format.rpm(root.row.rpm)
        color: root.nameColor
        font.family: root.mono
        font.weight: Theme.fonts.mono.weightBold
        font.pixelSize: Theme.wizard.rowRpm
      }

      // PUMP (step 1) or CONTROL (step 2).
      Rectangle {
        id: toggle
        visible: root.kind !== "readonly"
        readonly property bool on: root.kind === "pump" ? root.row.pump : root.row.control
        readonly property bool usable: !(root.kind === "pump" && root.row.locked)

        Layout.preferredWidth: Theme.wizard.toggleWidth
        Layout.preferredHeight: Theme.wizard.toggleHeight
        color: on ? Theme.colors.paper : Theme.colors.transparent
        border.width: on ? 0 : Theme.modes.border
        border.color: toggleMouse.containsMouse && usable ? Theme.colors.ash : Theme.colors.line
        opacity: usable ? 1 : 0.5

        Accessible.role: Accessible.CheckBox
        Accessible.name: (root.kind === "pump" ? "Pump: " : "Control: ") + Setup.outputName(root.row)
        Accessible.checkable: true
        Accessible.checked: on
        Accessible.onPressAction: if (toggle.usable) root.toggled()

        Behavior on color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }

        Text {
          anchors.centerIn: parent
          text: root.kind === "pump" ? "PUMP" : "CONTROL"
          color: toggle.on ? Theme.colors.ink : Theme.colors.paper
          font.family: root.stencil
          font.weight: Theme.fonts.stencil.weight
          font.pixelSize: Theme.wizard.toggleText
          font.letterSpacing: 1
        }

        MouseArea {
          id: toggleMouse
          anchors.fill: parent
          enabled: toggle.usable
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: root.toggled()
        }
      }
    }

    Text {
      Layout.fillWidth: true
      visible: root.row.locked && root.kind === "pump"
      wrapMode: Text.WordWrap
      text: "to change this, edit the config as root"
      color: Theme.colors.ash
      font.family: root.mono
      font.pixelSize: Theme.wizard.rowMeta
    }

    Text {
      Layout.fillWidth: true
      visible: root.kind === "fan" && !root.row.pump && Setup.noTachometer(root.row)
      wrapMode: Text.WordWrap
      text: "no tachometer reading. Is this header empty, or does its RPM come from another fanN?"
      color: Theme.colors.ash
      font.family: root.mono
      font.pixelSize: Theme.wizard.rowMeta
    }

    // The fan's settings, once CONTROL is on.
    ColumnLayout {
      Layout.fillWidth: true
      visible: root.expanded
      spacing: Theme.wizard.gap

      RowLayout {
        Layout.fillWidth: true
        spacing: Theme.wizard.gap

        Field {
          Layout.fillWidth: true
          Layout.preferredWidth: 3
          title: "LABEL"
          family: root.mono
          value: root.row.label
          onEdited: function(text) { root.fieldChanged("label", text) }
        }

        Field {
          Layout.fillWidth: true
          Layout.preferredWidth: 2
          title: "ID"
          family: root.mono
          value: root.row.id
          onEdited: function(text) { root.fieldChanged("id", text) }
        }
      }

      Choice {
        Layout.fillWidth: true
        title: "SENSOR"
        family: root.mono
        options: root.sensorOptions
        value: root.row.sensor
        onPicked: function(value) { root.fieldChanged("sensor", value) }
      }

      // Minimum %, sent on release.
      ColumnLayout {
        id: minimum
        Layout.fillWidth: true
        spacing: 4

        property real sliderValue: root.row.minPercent
        property bool sliding: false

        Text {
          text: "MINIMUM"
          color: Theme.colors.ash
          font.family: root.mono
          font.pixelSize: Theme.wizard.fieldLabel
          font.letterSpacing: 1
        }

        RowLayout {
          Layout.fillWidth: true
          spacing: 14

          Item {
            id: slider
            Layout.fillWidth: true
            Layout.preferredHeight: Theme.modes.sliderHeight

            readonly property real low: root.row.minLow
            readonly property real span: Math.max(1, root.row.minHigh - root.row.minLow)
            readonly property real shown: minimum.sliding ? minimum.sliderValue : root.row.minPercent
            readonly property real ratio: (shown - low) / span

            Accessible.role: Accessible.Slider
            Accessible.name: "Minimum speed " + Math.round(shown) + " percent"

            function valueAt(x) {
              var r = Math.min(1, Math.max(0, x / width))
              return Math.round(low + r * span)
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
              color: minimum.sliding ? Theme.colors.paper : Theme.colors.ink
              border.width: Theme.editor.pointStroke
              border.color: minimum.sliding ? Theme.colors.riot : Theme.colors.paper
            }

            MouseArea {
              anchors.fill: parent
              anchors.margins: -6
              cursorShape: Qt.PointingHandCursor
              preventStealing: true
              onPressed: function(mouse) {
                minimum.sliding = true
                minimum.sliderValue = slider.valueAt(mouse.x - 6)
              }
              onPositionChanged: function(mouse) {
                if (minimum.sliding) minimum.sliderValue = slider.valueAt(mouse.x - 6)
              }
              onReleased: {
                minimum.sliding = false
                root.fieldChanged("minPercent", Math.round(minimum.sliderValue))
              }
              onCanceled: minimum.sliding = false
            }
          }

          Text {
            Layout.preferredWidth: 52
            horizontalAlignment: Text.AlignRight
            text: Math.round(slider.shown) + "%"
            color: Theme.colors.paper
            font.family: root.mono
            font.weight: Theme.fonts.mono.weightBold
            font.pixelSize: Theme.panel.temps
          }
        }
      }

      // ADVANCED: the tachometer.
      Text {
        text: (root.row.advanced ? "▾ " : "▸ ") + "ADVANCED"
        color: advancedMouse.containsMouse ? Theme.colors.paper : Theme.colors.ash
        font.family: root.mono
        font.pixelSize: Theme.wizard.fieldLabel
        font.letterSpacing: 1

        Accessible.role: Accessible.Button
        Accessible.name: "Advanced settings"
        Accessible.onPressAction: root.fieldChanged("advanced", !root.row.advanced)

        MouseArea {
          id: advancedMouse
          anchors.fill: parent
          anchors.margins: -6
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: root.fieldChanged("advanced", !root.row.advanced)
        }
      }

      Choice {
        Layout.fillWidth: true
        visible: root.row.advanced
        title: "TACHOMETER (rpm_input)"
        family: root.mono
        options: root.rpmOptions
        value: root.row.rpmInput
        onPicked: function(value) { root.fieldChanged("rpmInput", value) }
      }
    }

    Text {
      Layout.fillWidth: true
      visible: root.problem !== "" && (root.expanded || root.isPump)
      wrapMode: Text.WordWrap
      text: "✕ " + root.problem
      color: Theme.colors.riot
      font.family: root.mono
      font.weight: Theme.fonts.mono.weightBold
      font.pixelSize: Theme.wizard.rowMeta
    }
  }

  // "HANDS OFF" tape across the top-right corner of a marked pump.
  Item {
    visible: root.isPump
    width: Theme.card.tapeWidth
    height: Theme.card.tapeHeight
    x: parent.width - width + Theme.card.padX / 2
    y: -height / 2
    rotation: Theme.card.tapeRotation

    Image {
      anchors.fill: parent
      source: Qt.resolvedUrl("../assets/tape-3.png")
      fillMode: Image.Stretch
      smooth: true
      mipmap: true
    }

    Text {
      anchors.centerIn: parent
      text: "HANDS OFF"
      color: Theme.colors.ink
      font.family: root.mono
      font.weight: Theme.fonts.mono.weightBold
      font.pixelSize: Theme.card.tapeText
      font.letterSpacing: Theme.card.tapeTracking
    }
  }

  // A one-line text field in a dashed box.
  component Field: ColumnLayout {
    id: field

    property string title: ""
    property string value: ""
    property string family: Theme.fonts.mono.family
    signal edited(string text)

    spacing: 4

    Text {
      text: field.title
      color: Theme.colors.ash
      font.family: field.family
      font.pixelSize: Theme.wizard.fieldLabel
      font.letterSpacing: 1
    }

    Item {
      Layout.fillWidth: true
      Layout.preferredHeight: Theme.wizard.fieldHeight

      Rectangle {
        anchors.fill: parent
        color: Theme.colors.inkRaised
      }

      DashedBox {
        anchors.fill: parent
        strokeColor: input.activeFocus ? Theme.colors.paper : Theme.colors.ash
      }

      TextInput {
        id: input
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        verticalAlignment: TextInput.AlignVCenter
        clip: true
        text: field.value
        color: Theme.colors.paper
        selectionColor: Theme.colors.riot
        selectedTextColor: Theme.colors.paper
        font.family: field.family
        font.pixelSize: Theme.wizard.field
        maximumLength: 48
        selectByMouse: true
        Accessible.role: Accessible.EditableText
        Accessible.name: field.title
        onTextEdited: field.edited(input.text)
      }
    }
  }

  // A choice from a short list, opened in place (no popup window).
  component Choice: ColumnLayout {
    id: choice

    property string title: ""
    property string family: Theme.fonts.mono.family
    property var options: []
    property var value: null
    property bool open: false
    signal picked(var value)

    spacing: 4

    Text {
      text: choice.title
      color: Theme.colors.ash
      font.family: choice.family
      font.pixelSize: Theme.wizard.fieldLabel
      font.letterSpacing: 1
    }

    Rectangle {
      Layout.fillWidth: true
      Layout.preferredHeight: Theme.wizard.fieldHeight
      color: Theme.colors.inkRaised
      border.width: 1
      border.color: currentMouse.containsMouse || choice.open ? Theme.colors.ash : Theme.colors.line

      Accessible.role: Accessible.ComboBox
      Accessible.name: choice.title + ": " + Setup.optionLabel(choice.options, choice.value)
      Accessible.onPressAction: choice.open = !choice.open

      Text {
        anchors.verticalCenter: parent.verticalCenter
        x: 10
        width: parent.width - 40
        elide: Text.ElideRight
        text: Setup.optionLabel(choice.options, choice.value)
        color: Theme.colors.paper
        font.family: choice.family
        font.pixelSize: Theme.wizard.field
      }

      Text {
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: 10
        text: choice.open ? "▴" : "▾"
        color: Theme.colors.ash
        font.family: choice.family
        font.pixelSize: Theme.wizard.field
      }

      MouseArea {
        id: currentMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: choice.open = !choice.open
      }
    }

    Column {
      Layout.fillWidth: true
      visible: choice.open

      Repeater {
        model: choice.options

        Rectangle {
          id: option
          required property var modelData
          readonly property bool current: modelData.value === choice.value

          width: parent.width
          height: Theme.wizard.optionHeight
          color: current ? Theme.colors.paper
                 : (optionMouse.containsMouse ? Theme.colors.line : Theme.colors.inkRaised)

          Accessible.role: Accessible.ListItem
          Accessible.name: option.modelData.label
          Accessible.onPressAction: {
            choice.open = false
            choice.picked(option.modelData.value)
          }

          Text {
            anchors.verticalCenter: parent.verticalCenter
            x: 10
            width: parent.width - 20
            elide: Text.ElideRight
            text: option.modelData.label
            color: option.current ? Theme.colors.ink : Theme.colors.paper
            font.family: choice.family
            font.pixelSize: Theme.wizard.field
          }

          MouseArea {
            id: optionMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
              choice.open = false
              choice.picked(option.modelData.value)
            }
          }
        }
      }
    }
  }
}
