pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import "../models/theme.js" as Theme
import "../models/curve.js" as Curve
import "../models/setup.js" as Setup

// The setup wizard (specs/08-setup-wizard.md): 1 mark the pump, 2 pick
// your fans, 3 check and apply. Draws a setup.js draft and emits what the
// user did; Panel.qml runs it through setup.js and owns the draft. Nothing
// is written before APPLY.
ColumnLayout {
  id: root

  property var draft: null             // setup.js draft
  property var starter: []             // list_outputs starter_curve
  property var config: null            // get_config reply
  property var status: null            // live status (sensor and tachometer choices)
  property var fonts: null
  property string errorText: ""        // the daemon's message, as is
  property bool busy: false            // APPLY sent, waiting for the reply

  signal pumpToggled(string key)
  signal noPumpChosen()
  signal controlToggled(string key)
  signal fieldChanged(string key, string field, var value)
  signal backRequested()
  signal nextRequested()
  signal applyRequested()

  readonly property int step: draft ? draft.step : 1
  readonly property int rowCount: draft ? draft.rows.length : 0
  readonly property var problems: draft ? Setup.validateDraft(draft) : ({})
  readonly property var sensors: Setup.sensorOptions(config, status)
  readonly property var summary: draft ? Setup.summary(draft, starter) : ({ fans: [], pumps: [] })
  readonly property string stencil: fonts ? fonts.stencil : Theme.fonts.stencil.family
  readonly property string mono: fonts ? fonts.mono : Theme.fonts.mono.family
  readonly property string note: fonts ? fonts.note : Theme.fonts.note.family

  spacing: Theme.panel.section

  function rowAt(index) {
    return root.draft && index < root.draft.rows.length ? root.draft.rows[index] : null
  }

  // Header: "STEP 1 / 3" and the step title.
  ColumnLayout {
    Layout.fillWidth: true
    spacing: 2

    Text {
      text: Setup.stepHeader(root.step)
      color: Theme.colors.ash
      font.family: root.mono
      font.pixelSize: Theme.wizard.stepLabel
      font.letterSpacing: 1
    }

    Text {
      Layout.fillWidth: true
      wrapMode: Text.WordWrap
      text: Setup.stepTitle(root.step)
      color: Theme.colors.paper
      font.family: root.stencil
      font.weight: Theme.fonts.stencil.weight
      font.pixelSize: Theme.wizard.title
      font.letterSpacing: 1
    }
  }

  Text {
    Layout.fillWidth: true
    visible: root.draft !== null && root.rowCount === 0
    wrapMode: Text.WordWrap
    text: root.draft ? "No PWM outputs on the chips in the config." : "Reading the outputs…"
    color: Theme.colors.ash
    font.family: root.mono
    font.pixelSize: Theme.footer.text
  }

  // ------------------------------------------------------------ step 1

  ColumnLayout {
    Layout.fillWidth: true
    visible: root.step === 1
    spacing: Theme.card.gap + Theme.card.tapeHeight / 3

    // Delegates are kept across draft updates (model is a count).
    Repeater {
      model: root.step === 1 ? root.rowCount : 0

      OutputRow {
        required property int index
        Layout.fillWidth: true
        row: root.rowAt(index)
        kind: "pump"
        fonts: root.fonts
        onToggled: root.pumpToggled(row.key)
      }
    }

    Text {
      Layout.fillWidth: true
      Layout.topMargin: 4
      wrapMode: Text.WordWrap
      text: "A pump that stops can cook your CPU. Mark it first. Fanarchy will never touch it."
      color: Theme.colors.ash
      font.family: root.note
      font.pixelSize: Theme.wizard.note
    }

    Rectangle {
      id: noPump
      visible: root.draft !== null && !Setup.hasLockedPump(root.draft)
      readonly property bool chosen: root.draft !== null && root.draft.noPump

      Layout.preferredWidth: Math.max(180, noPumpLabel.implicitWidth + 40)
      Layout.preferredHeight: Theme.wizard.navHeight
      color: chosen ? Theme.colors.paper : Theme.colors.transparent
      border.width: chosen ? 0 : Theme.modes.border
      border.color: noPumpMouse.containsMouse ? Theme.colors.ash : Theme.colors.line

      Accessible.role: Accessible.Button
      Accessible.name: "I have no pump"
      Accessible.onPressAction: root.noPumpChosen()

      Text {
        id: noPumpLabel
        anchors.centerIn: parent
        text: noPump.chosen ? "✓ I HAVE NO PUMP" : "I HAVE NO PUMP"
        color: noPump.chosen ? Theme.colors.ink : Theme.colors.paper
        font.family: root.stencil
        font.weight: Theme.fonts.stencil.weight
        font.pixelSize: Theme.wizard.toggleText + 2
        font.letterSpacing: 1
      }

      MouseArea {
        id: noPumpMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.noPumpChosen()
      }
    }
  }

  // ------------------------------------------------------------ step 2

  ColumnLayout {
    Layout.fillWidth: true
    visible: root.step === 2
    spacing: Theme.card.gap + Theme.card.tapeHeight / 3

    // Pumps first, read-only.
    Repeater {
      model: root.step === 2 ? root.rowCount : 0

      OutputRow {
        required property int index
        Layout.fillWidth: true
        visible: row !== null && row.pump
        row: root.rowAt(index)
        kind: "readonly"
        fonts: root.fonts
      }
    }

    Repeater {
      model: root.step === 2 ? root.rowCount : 0

      OutputRow {
        required property int index
        Layout.fillWidth: true
        visible: row !== null && !row.pump
        row: root.rowAt(index)
        kind: "fan"
        fonts: root.fonts
        problem: row && root.problems[row.key] ? root.problems[row.key] : ""
        sensorOptions: root.sensors
        rpmOptions: row ? Setup.rpmInputOptions(root.config, root.status, row.chip) : []
        onToggled: root.controlToggled(row.key)
        onFieldChanged: function(field, value) { root.fieldChanged(row.key, field, value) }
      }
    }
  }

  // ------------------------------------------------------------ step 3

  ColumnLayout {
    Layout.fillWidth: true
    visible: root.step === 3
    spacing: Theme.card.gap

    Text {
      Layout.fillWidth: true
      visible: root.summary.fans.length === 0
      wrapMode: Text.WordWrap
      text: "No fans picked. Every output stays with the BIOS."
      color: Theme.colors.ash
      font.family: root.mono
      font.pixelSize: Theme.footer.text
    }

    Repeater {
      model: root.step === 3 ? root.summary.fans : []

      Rectangle {
        id: fanSummary
        required property var modelData

        Layout.fillWidth: true
        implicitHeight: Math.max(Theme.wizard.previewHeight, summaryText.implicitHeight) + Theme.card.padY * 2
        color: Theme.colors.transparent
        border.width: Theme.card.border
        border.color: Theme.colors.line

        Accessible.role: Accessible.StaticText
        Accessible.name: fanSummary.modelData.name + ", " + fanSummary.modelData.line

        ColumnLayout {
          id: summaryText
          anchors.verticalCenter: parent.verticalCenter
          x: Theme.card.padX
          width: parent.width - Theme.card.padX * 3 - Theme.wizard.previewWidth
          spacing: 2

          Text {
            Layout.fillWidth: true
            elide: Text.ElideRight
            text: fanSummary.modelData.name
            color: Theme.colors.paper
            font.family: root.stencil
            font.weight: Theme.fonts.stencil.weight
            font.pixelSize: Theme.card.name
          }

          Text {
            Layout.fillWidth: true
            elide: Text.ElideRight
            text: fanSummary.modelData.line
            color: Theme.colors.ash
            font.family: root.mono
            font.pixelSize: Theme.card.meta
          }
        }

        // Read-only preview of the curve the fan starts with.
        Rectangle {
          id: preview
          anchors.verticalCenter: parent.verticalCenter
          anchors.right: parent.right
          anchors.rightMargin: Theme.card.padX
          width: Theme.wizard.previewWidth
          height: Theme.wizard.previewHeight
          color: Theme.colors.inkRaised

          Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
              strokeColor: Theme.colors.paper
              strokeWidth: Theme.wizard.previewLine
              fillColor: "transparent"
              joinStyle: ShapePath.RoundJoin
              PathPolyline {
                path: Curve.polyline(fanSummary.modelData.curve, preview.width, preview.height).map(function(p) {
                  return Qt.point(p.x, p.y)
                })
              }
            }
          }
        }
      }
    }

    Repeater {
      model: root.step === 3 ? root.summary.pumps : []

      Item {
        id: pumpSummary
        required property var modelData

        Layout.fillWidth: true
        implicitHeight: pumpColumn.implicitHeight + Theme.card.padY * 2

        DashedBox {
          anchors.fill: parent
          strokeColor: Theme.colors.ash
        }

        ColumnLayout {
          id: pumpColumn
          x: Theme.card.padX
          y: Theme.card.padY
          width: parent.width - Theme.card.padX * 2
          spacing: 2

          Text {
            text: pumpSummary.modelData.name
            color: Theme.colors.ash
            font.family: root.stencil
            font.weight: Theme.fonts.stencil.weight
            font.pixelSize: Theme.card.name
          }

          Text {
            text: pumpSummary.modelData.line + " · never touched"
            color: Theme.colors.ash
            font.family: root.mono
            font.pixelSize: Theme.card.meta
          }
        }
      }
    }

    Text {
      Layout.fillWidth: true
      Layout.topMargin: 4
      wrapMode: Text.WordWrap
      text: "You can change every curve after this. Fans you didn't pick stay with the BIOS."
      color: Theme.colors.ash
      font.family: root.note
      font.pixelSize: Theme.wizard.note
    }
  }

  // The daemon's message when it refuses the setup.
  Rectangle {
    Layout.fillWidth: true
    visible: root.errorText !== ""
    implicitHeight: errorLabel.implicitHeight + 20
    color: Theme.colors.inkRaised
    border.width: Theme.card.border
    border.color: Theme.colors.riot

    Text {
      id: errorLabel
      anchors.verticalCenter: parent.verticalCenter
      x: 12
      width: parent.width - 24
      wrapMode: Text.WordWrap
      text: "✕ " + root.errorText
      color: Theme.colors.paper
      font.family: root.mono
      font.pixelSize: Theme.footer.text
    }
  }

  // BACK and NEXT / APPLY.
  RowLayout {
    Layout.fillWidth: true
    spacing: Theme.modes.gap

    NavButton {
      Layout.fillWidth: true
      Layout.preferredWidth: 1
      label: root.step === 1 ? "CANCEL" : "BACK"
      family: root.stencil
      active: !root.busy
      onActivated: root.backRequested()
    }

    NavButton {
      Layout.fillWidth: true
      Layout.preferredWidth: 1
      label: root.step === 3 ? (root.busy ? "APPLYING…" : "APPLY") : "NEXT"
      family: root.stencil
      primary: root.step === 3
      active: root.step === 3 ? !root.busy : Setup.canAdvance(root.draft)
      onActivated: root.step === 3 ? root.applyRequested() : root.nextRequested()
    }
  }

  component NavButton: Rectangle {
    id: button

    property string label: ""
    property string family: Theme.fonts.stencil.family
    property bool primary: false
    property bool active: true
    signal activated()

    Layout.preferredHeight: Theme.wizard.navHeight
    color: primary ? Theme.colors.riot : (buttonMouse.containsMouse && active ? Theme.colors.inkRaised : Theme.colors.transparent)
    border.width: primary ? 0 : Theme.modes.border
    border.color: buttonMouse.containsMouse && active ? Theme.colors.ash : Theme.colors.line
    opacity: active ? 1 : 0.4
    scale: buttonMouse.pressed ? Theme.bar.pressScale : 1

    Accessible.role: Accessible.Button
    Accessible.name: button.label
    Accessible.onPressAction: if (button.active) button.activated()

    Behavior on scale { NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }

    Text {
      anchors.centerIn: parent
      text: button.label
      color: button.primary ? Theme.colors.ink : Theme.colors.paper
      font.family: button.family
      font.weight: Theme.fonts.stencil.weight
      font.pixelSize: Theme.wizard.navText
      font.letterSpacing: 1
    }

    MouseArea {
      id: buttonMouse
      anchors.fill: parent
      enabled: button.active
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: button.activated()
    }
  }
}
