import QtQuick
import QtQuick.Layouts
import "../models/theme.js" as Theme

// The pump: read-only, never selectable, never controlled. Dashed ash
// frame and a "HANDS OFF" tape on the top-right corner.
Item {
  id: root

  required property var card      // panel.js fanCard() for a pump
  property var fonts: null

  implicitHeight: Math.max(Theme.card.minHeight, content.implicitHeight + Theme.card.padY * 2)

  Accessible.role: Accessible.StaticText
  Accessible.name: card.name + ", " + card.header + ", " + card.rpm

  DashedBox {
    anchors.fill: parent
    strokeColor: Theme.colors.ash
    strokeWidth: 1
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
        color: Theme.colors.ash
        font.family: root.fonts ? root.fonts.stencil : Theme.fonts.stencil.family
        font.weight: Theme.fonts.stencil.weight
        font.pixelSize: Theme.card.name
      }

      Text {
        Layout.fillWidth: true
        text: root.card.header
        elide: Text.ElideRight
        color: Theme.colors.ash
        font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
        font.pixelSize: Theme.card.meta
      }
    }

    ColumnLayout {
      spacing: 2

      Text {
        Layout.alignment: Qt.AlignRight
        text: root.card.rpm
        color: Theme.colors.ash
        font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
        font.weight: Theme.fonts.mono.weightBold
        font.pixelSize: Theme.card.rpm
      }

      Text {
        Layout.alignment: Qt.AlignRight
        text: root.card.detail
        color: Theme.colors.ash
        font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
        font.pixelSize: Theme.card.meta
      }
    }
  }

  // "HANDS OFF" tape across the top-right corner.
  Item {
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
      font.family: root.fonts ? root.fonts.mono : Theme.fonts.mono.family
      font.weight: Theme.fonts.mono.weightBold
      font.pixelSize: Theme.card.tapeText
      font.letterSpacing: Theme.card.tapeTracking
    }
  }
}
