import QtQuick
import QtQuick.Layouts
import "../models/theme.js" as Theme

// One controlled fan: name and header on the left, RPM and speed on the
// right. The whole card is a button that selects the fan.
Item {
  id: root

  required property var card      // panel.js fanCard()
  property var fonts: null
  property bool selected: false
  property bool interactive: true

  signal clicked()

  readonly property color textColor: selected ? Theme.colors.ink : Theme.colors.paper
  readonly property color metaColor: selected ? Theme.colors.ink : Theme.colors.ash

  implicitHeight: Math.max(Theme.card.minHeight, content.implicitHeight + Theme.card.padY * 2)

  Accessible.role: Accessible.Button
  Accessible.name: card.name + ", " + card.rpm + ", " + card.detail
  Accessible.checkable: true
  Accessible.checked: selected
  Accessible.onPressAction: if (root.interactive) root.clicked()

  Rectangle {
    anchors.fill: parent
    color: root.selected ? Theme.colors.paper : Theme.colors.transparent
    border.width: root.selected ? 0 : Theme.card.border
    border.color: mouse.containsMouse && root.interactive ? Theme.colors.ash : Theme.colors.line

    Behavior on color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
    Behavior on border.color { ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad } }
  }

  RowLayout {
    id: content
    anchors.fill: parent
    anchors.leftMargin: Theme.card.padX
    anchors.rightMargin: Theme.card.padX
    anchors.topMargin: Theme.card.padY
    anchors.bottomMargin: Theme.card.padY
    spacing: Theme.card.padX

    ColumnLayout {
      Layout.fillWidth: true
      spacing: 2

      Text {
        Layout.fillWidth: true
        text: root.card.name
        elide: Text.ElideRight
        color: root.textColor
        font.family: root.fonts ? root.fonts.stencil : Theme.fonts.stencil.family
        font.weight: Theme.fonts.stencil.weight
        font.pixelSize: Theme.card.name
        Behavior on color { ColorAnimation { duration: Theme.motion.duration } }
      }

      Text {
        Layout.fillWidth: true
        text: root.card.header
        elide: Text.ElideRight
        color: root.metaColor
        font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
        font.pixelSize: Theme.card.meta
        Behavior on color { ColorAnimation { duration: Theme.motion.duration } }
      }
    }

    ColumnLayout {
      spacing: 2

      Text {
        Layout.alignment: Qt.AlignRight
        text: root.card.rpm
        color: root.textColor
        font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
        font.weight: Theme.fonts.mono.weightBold
        font.pixelSize: Theme.card.rpm
        Behavior on color { ColorAnimation { duration: Theme.motion.duration } }
      }

      Text {
        Layout.alignment: Qt.AlignRight
        text: root.card.detail
        color: root.metaColor
        font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
        font.pixelSize: Theme.card.meta
        Behavior on color { ColorAnimation { duration: Theme.motion.duration } }
      }
    }
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    enabled: root.interactive
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: root.clicked()
  }
}
