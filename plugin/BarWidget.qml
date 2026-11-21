import QtQuick
import QtQuick.Shapes
import qs.Ui as Ui
import "models/theme.js" as Theme
import "models/FanarchyClient.js" as Client
import "components"

// Fanarchy bar widget: the mark plus the CPU temperature. Three looks:
// normal, running hot and daemon down (specs/04-bar-widget.md). All state
// comes from FanarchyConnection; this file only draws it.
Ui.BarWidget {
  id: root
  moduleName: Client.PLUGIN_ID

  readonly property var view: connection.view
  readonly property bool hot: view.state === "hot"
  readonly property bool down: view.state === "down"
  property var registeredBar: null

  implicitWidth: vertical ? Math.max(barSize, button.width) : button.width
  implicitHeight: vertical ? button.height + Theme.bar.paddingX : barSize

  function togglePanel() {
    tooltip.open = false
    if (root.bar) root.bar.run(Client.togglePanelCommand())
  }

  // The bar only routes clicks to registered targets, as WidgetButton does.
  function syncClickRegistration() {
    if (registeredBar && registeredBar.unregisterClickTarget) registeredBar.unregisterClickTarget(button)
    registeredBar = root.bar
    if (registeredBar && registeredBar.registerClickTarget) registeredBar.registerClickTarget(button)
  }

  onBarChanged: syncClickRegistration()
  Component.onCompleted: syncClickRegistration()
  Component.onDestruction: {
    if (registeredBar && registeredBar.unregisterClickTarget) registeredBar.unregisterClickTarget(button)
  }

  FontLoader { id: stencilFont; source: Qt.resolvedUrl("assets/fonts/" + Theme.fonts.stencil.files[0]) }
  FontLoader { id: monoRegular; source: Qt.resolvedUrl("assets/fonts/" + Theme.fonts.mono.files[0]) }
  FontLoader { id: monoBold; source: Qt.resolvedUrl("assets/fonts/" + Theme.fonts.mono.files[1]) }
  FontLoader { id: noteFont; source: Qt.resolvedUrl("assets/fonts/" + Theme.fonts.note.files[0]) }

  FanarchyConnection {
    id: connection
  }

  Item {
    id: button
    anchors.centerIn: parent
    width: row.implicitWidth + Theme.bar.paddingX * 2
    height: Theme.bar.height
    scale: mouse.pressed ? Theme.bar.pressScale : 1

    Accessible.role: Accessible.Button
    Accessible.name: root.view.accessibleName
    Accessible.onPressAction: root.togglePanel()

    Behavior on scale {
      NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad }
    }

    // Solid frame: paper outline when normal, riot block when hot.
    Rectangle {
      anchors.fill: parent
      color: root.hot ? Theme.colors.riot : Theme.colors.transparent
      border.width: root.down ? 0 : Theme.bar.border
      border.color: root.hot ? Theme.colors.riot : Theme.colors.paper
      antialiasing: true

      Behavior on color {
        ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad }
      }
    }

    // Dashed ash frame when the daemon is down.
    Shape {
      anchors.fill: parent
      visible: root.down

      ShapePath {
        strokeColor: Theme.colors.ash
        strokeWidth: Theme.bar.border
        strokeStyle: ShapePath.DashLine
        dashPattern: [Theme.bar.dash, Theme.bar.dash]
        fillColor: Theme.colors.transparent
        PathRectangle {
          x: 0.5
          y: 0.5
          width: button.width - 1
          height: button.height - 1
        }
      }
    }

    // Hover wash in the text color.
    Rectangle {
      anchors.fill: parent
      anchors.margins: Theme.bar.border
      color: root.hot ? Theme.colors.ink : (root.down ? Theme.colors.ash : Theme.colors.paper)
      opacity: mouse.containsMouse ? Theme.bar.hoverWash : 0

      Behavior on opacity {
        NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad }
      }
    }

    Row {
      id: row
      anchors.centerIn: parent
      spacing: Theme.bar.gap

      FanMark {
        anchors.verticalCenter: parent.verticalCenter
        size: Theme.bar.icon
        slashed: root.down
        ringColor: root.hot ? Theme.colors.ink : (root.down ? Theme.colors.ash : Theme.colors.riot)
        fanColor: root.hot ? Theme.colors.ink : Theme.colors.paper
      }

      Text {
        anchors.verticalCenter: parent.verticalCenter
        text: root.view.label
        textFormat: Text.PlainText
        color: root.hot ? Theme.colors.ink : (root.down ? Theme.colors.ash : Theme.colors.paper)
        font.family: monoBold.name !== "" ? monoBold.name : Theme.fonts.mono.family
        font.weight: Theme.fonts.mono.weightBold
        font.pixelSize: root.hot ? Theme.bar.numberHot : Theme.bar.number

        Behavior on color {
          ColorAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad }
        }
      }
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      acceptedButtons: Qt.LeftButton
      cursorShape: Qt.PointingHandCursor
      onClicked: root.togglePanel()
    }
  }

  FanarchyTooltip {
    id: tooltip
    anchorItem: button
    bar: root.bar
    view: root.view
    open: mouse.containsMouse && !mouse.pressed
    stencilFamily: stencilFont.name !== "" ? stencilFont.name : Theme.fonts.stencil.family
    monoFamily: monoRegular.name !== "" ? monoRegular.name : Theme.fonts.mono.family
    noteFamily: noteFont.name !== "" ? noteFont.name : Theme.fonts.note.family
  }
}
