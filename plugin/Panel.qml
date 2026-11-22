pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland
import "models/theme.js" as Theme
import "models/panel.js" as Panel
import "models/curve.js" as Curve
import "models/FanarchyClient.js" as Client
import "components"

// Fanarchy panel: a drawer on the right edge with one card per fan, the
// curve editor and the mode selector (specs/05-panel-and-curve-editor.md).
// Decisions live in models/; this file wires them to the UI.
Item {
  id: root

  property var shell: null
  property var manifest: null
  property bool opened: false

  property string selectedId: ""
  property var config: null               // get_config reply
  property var draftPoints: []            // curve shown in the editor
  property bool curveInFlight: false
  property var pendingMode: null          // { fan, mode } until the status reports it
  property string confirmText: ""
  property string errorText: ""
  property real lastDismissMs: -1
  property string screenName: ""          // monitor whose widget opened the panel

  readonly property var clientState: live.clientState
  readonly property var status: clientState.status
  readonly property string panelState: Panel.panelState(clientState.connected, status)
  readonly property var selectedFan: Panel.statusFan(status, selectedId)
  readonly property var savedCurve: Panel.curveOf(config, selectedId)
  readonly property var cards: panelState === "pending" ? Panel.placeholderCards() : Panel.fanCards(status, config)
  readonly property var temps: Panel.headerTemps(status)

  // ------------------------------------------------------------ shell API

  // Omarchy's lifecycle owns the open state: shell.summon/toggle call
  // open(), shell.hide/toggle call close(), and shell.isPluginOpen reads
  // `opened`. Nothing else writes it.
  function open(payloadJson) {
    var guard = Panel.reopenGuard(root.lastDismissMs, Date.now())
    root.lastDismissMs = guard.nextLastDismissMs
    if (guard.ignore) {
      // The click that dismissed the panel also reached the widget.
      console.warn("[fanarchy] panel open ignored (same click as the dismiss)")
      Qt.callLater(root.hideFromShell)
      return
    }
    if (root.opened) return
    var focused = Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : ""
    root.screenName = Panel.screenFromPayload(payloadJson) || focused
    console.warn("[fanarchy] panel open on " + (root.screenName || "default screen"))
    root.opened = true
  }

  function close() {
    if (!root.opened) return
    console.warn("[fanarchy] panel close")
    root.opened = false
    root.errorText = ""
    root.confirmText = ""
  }

  function toggle() {
    if (root.opened) root.dismiss("toggle")
    else root.open("")
  }

  // Through Omarchy's API so its openPanelIds stays in step; close()
  // directly only when the host gave us no shell.
  function hideFromShell() {
    if (root.shell && typeof root.shell.hide === "function") root.shell.hide(Client.PLUGIN_ID)
    else root.close()
  }

  // Every close the user triggers from inside the panel.
  function dismiss(reason) {
    if (!root.opened) return
    console.warn("[fanarchy] panel dismiss (" + reason + ")")
    root.lastDismissMs = Date.now()
    root.hideFromShell()
  }

  // ----------------------------------------------------------- actions

  function resetDraft() {
    if (!root.curveInFlight) root.draftPoints = Curve.sorted(root.savedCurve)
  }

  function applyCurve(points) {
    if (!root.selectedFan) return
    var problem = Curve.validate(points, Panel.minPercent(root.selectedFan))
    if (problem) {
      root.showError(problem)
      root.resetDraft()
      return
    }
    root.draftPoints = points
    root.curveInFlight = true
    commands.send(Client.setCurveRequest(root.selectedId, points))
  }

  function requestMode(mode) {
    if (!root.selectedFan) return
    var percent = mode === "fixed" ? Panel.fixedStart(root.selectedFan) : null
    root.pendingMode = { fan: root.selectedId, mode: mode }
    commands.send(Client.setModeRequest(root.selectedId, mode, percent))
  }

  function requestFixed(percent) {
    if (!root.selectedFan) return
    root.pendingMode = { fan: root.selectedId, mode: "fixed" }
    commands.send(Client.setModeRequest(root.selectedId, "fixed", percent))
  }

  function copyCommand(command) {
    Quickshell.clipboardText = command
    root.showConfirmation(Panel.confirmation({ cmd: "copy" }))
  }

  function showConfirmation(text) {
    root.errorText = ""
    root.confirmText = text
    confirmTimer.restart()
  }

  function showError(text) {
    root.confirmText = ""
    root.errorText = text
  }

  function handleReply(request, ok, data, error) {
    if (request.cmd === "get_config") {
      if (ok) {
        root.config = data
        root.resetDraft()
      } else {
        root.showError(error)
      }
      return
    }
    if (request.cmd === "set_curve") {
      root.curveInFlight = false
      if (ok) {
        root.config = Panel.withCurve(root.config, request.fan, request.points)
        root.showConfirmation(Panel.confirmation(request))
      } else {
        root.showError(error)
      }
      root.resetDraft()
      return
    }
    if (request.cmd === "set_mode") {
      if (ok) {
        root.showConfirmation(Panel.confirmation(request))
      } else {
        root.pendingMode = null
        root.showError(error)
      }
    }
  }

  onStatusChanged: {
    root.selectedId = Panel.pickSelection(root.status, root.selectedId)
    if (Panel.pendingSettled(root.selectedFan, root.pendingMode)) root.pendingMode = null
  }
  onSelectedIdChanged: root.resetDraft()
  onSavedCurveChanged: root.resetDraft()
  onOpenedChanged: if (!opened) root.pendingMode = null

  Timer {
    id: confirmTimer
    interval: Theme.footer.confirmMs
    onTriggered: root.confirmText = ""
  }

  // ------------------------------------------------------- connections

  FanarchyConnection {
    id: live
    active: root.opened
  }

  FanarchyCommands {
    id: commands
    active: root.opened
    onReady: commands.send(Client.getConfigRequest())
    onReplied: function(request, ok, data, error) { root.handleReply(request, ok, data, error) }
  }

  Fonts {
    id: fontSet
  }

  // ------------------------------------------------------------- window

  // A fresh window (and focus grab) on every open, destroyed on close, so
  // no binding on `visible` or the grab's `active` can go stale.
  LazyLoader {
    active: root.opened

    PanelWindow {
      id: window

      screen: Panel.pickScreen(Quickshell.screens, root.screenName, "")
      anchors { top: true; right: true; bottom: true }
      implicitWidth: Theme.panel.width
      color: "transparent"
      exclusionMode: ExclusionMode.Normal
      WlrLayershell.namespace: "fanarchy-panel"
      WlrLayershell.layer: WlrLayer.Top
      WlrLayershell.keyboardFocus: root.opened ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

      // The open state follows the real window: if the compositor drops it
      // while the panel is marked open, close through the shell.
      onVisibleChanged: {
        if (Panel.windowLost(root.opened, window.visible)) root.dismiss("window lost")
        else window.armGrab()
      }
      Component.onCompleted: window.armGrab()

      // Start the grab a tick after the window shows, so it can't clear
      // itself before the surface is mapped. The timer dies with the window.
      function armGrab() { grabArm.restart() }

      Timer {
        id: grabArm
        interval: 0
        onTriggered: if (root.opened && window.visible) grab.active = true
      }

      // A click anywhere else closes the drawer.
      HyprlandFocusGrab {
        id: grab
        active: false
        windows: [window]
        onCleared: root.dismiss("outside")
      }

      Rectangle {
        anchors.fill: parent
        color: Theme.colors.ink
      }

      Image {
        anchors.fill: parent
        source: Qt.resolvedUrl("assets/paper-grain.png")
        sourceSize.width: Theme.panel.grain
        sourceSize.height: Theme.panel.grain
        fillMode: Image.Tile
      }

      Rectangle {
        width: Theme.panel.edge
        height: parent.height
        color: Theme.colors.riot
      }

      Item {
        id: keys
        anchors.fill: parent
        focus: true
        Keys.onEscapePressed: root.dismiss("esc")
      }

      Flickable {
        id: scroller
        anchors.fill: parent
        anchors.leftMargin: Theme.panel.edge
        contentWidth: width
        contentHeight: content.implicitHeight + Theme.panel.padTop * 2
        boundsBehavior: Flickable.StopAtBounds
        clip: true

        ColumnLayout {
          id: content
          x: Theme.panel.padSide
          y: Theme.panel.padTop
          width: scroller.width - Theme.panel.padSide * 2
          spacing: Theme.panel.section

          // Header: logo, wordmark, CPU and GPU.
          RowLayout {
            Layout.fillWidth: true
            spacing: 12

            Image {
              source: Qt.resolvedUrl("assets/logo.svg")
              sourceSize.width: Theme.panel.logo
              sourceSize.height: Theme.panel.logo
              Layout.preferredWidth: Theme.panel.logo
              Layout.preferredHeight: Theme.panel.logo
            }

            Text {
              text: "FANARCHY"
              color: Theme.colors.paper
              font.family: fontSet.stencil
              font.weight: Theme.fonts.stencil.weight
              font.pixelSize: Theme.panel.title
              font.letterSpacing: 1
            }

            Item { Layout.fillWidth: true }

            Text {
              text: "CPU <b>" + root.temps.cpu + "</b> · GPU <b>" + root.temps.gpu + "</b>"
              textFormat: Text.StyledText
              color: Theme.colors.paper
              font.family: fontSet.mono
              font.pixelSize: Theme.panel.temps
            }
          }

          // Daemon down or no fans: one empty state instead of the layout.
          EmptyState {
            id: emptyState
            Layout.fillWidth: true
            visible: root.panelState === "down" || root.panelState === "empty"
            fonts: fontSet
            copy: Panel.emptyState(root.panelState, live.view.downReason)
            onAction: {
              root.copyCommand(emptyState.copy.command)
              emptyState.flashDone()
            }
          }

          // Fans.
          ColumnLayout {
            Layout.fillWidth: true
            visible: root.panelState === "ready" || root.panelState === "pending"
            spacing: Theme.card.gap

            Repeater {
              model: Panel.cardsOfKind(root.cards, "fan")

              FanCard {
                required property var modelData
                Layout.fillWidth: true
                card: modelData
                fonts: fontSet
                interactive: root.panelState === "ready"
                selected: root.panelState === "ready" && modelData.id === root.selectedId
                onClicked: root.selectedId = modelData.id
              }
            }

            Repeater {
              model: Panel.cardsOfKind(root.cards, "pump")

              PumpCard {
                required property var modelData
                Layout.fillWidth: true
                Layout.topMargin: Theme.card.tapeHeight / 2
                card: modelData
                fonts: fontSet
              }
            }
          }

          CurveEditor {
            Layout.fillWidth: true
            visible: root.panelState === "ready" || root.panelState === "pending"
            fonts: fontSet
            interactive: root.panelState === "ready" && !root.curveInFlight
            title: Panel.editorTitle(root.selectedFan)
            sensor: root.selectedFan && root.selectedFan.sensor ? root.selectedFan.sensor : "—"
            points: root.draftPoints
            minPercent: Panel.minPercent(root.selectedFan)
            currentTemp: root.selectedFan ? root.selectedFan.temp_c : null
            criticalTemp: root.status ? root.status.critical_temp_c : 85
            onCommitted: function(points) { root.applyCurve(points) }
          }

          ModeSelector {
            Layout.fillWidth: true
            visible: root.panelState === "ready" || root.panelState === "pending"
            fonts: fontSet
            interactive: root.panelState === "ready"
            mode: Panel.displayMode(root.selectedFan, root.pendingMode)
            percent: Panel.fixedStart(root.selectedFan)
            minPercent: Panel.minPercent(root.selectedFan)
            onModeRequested: function(mode) { root.requestMode(mode) }
            onFixedRequested: function(percent) { root.requestFixed(percent) }
          }

          // The daemon's message, as is, when it refuses a change.
          Rectangle {
            Layout.fillWidth: true
            visible: root.errorText !== ""
            implicitHeight: errorLabel.implicitHeight + 20
            color: Theme.colors.inkRaised

            Rectangle {
              width: Theme.panel.edge
              height: parent.height
              color: Theme.colors.riot
            }

            Text {
              id: errorLabel
              anchors.verticalCenter: parent.verticalCenter
              x: Theme.panel.edge + 12
              width: parent.width - x - 12
              wrapMode: Text.WordWrap
              text: "✕ " + root.errorText
              color: Theme.colors.paper
              font.family: fontSet.mono
              font.pixelSize: Theme.footer.text
            }
          }

          // Footer: dashed divider, socket state, confirmation.
          ColumnLayout {
            Layout.fillWidth: true
            spacing: 10

            Shape {
              Layout.fillWidth: true
              Layout.preferredHeight: 1
              ShapePath {
                strokeColor: Theme.colors.line
                strokeWidth: 1
                strokeStyle: ShapePath.DashLine
                dashPattern: [Theme.editor.dash, Theme.editor.dash]
                fillColor: "transparent"
                startX: 0
                startY: 0.5
                PathLine { x: content.width; y: 0.5 }
              }
            }

            RowLayout {
              Layout.fillWidth: true

              Text {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: Panel.footerText(root.clientState.connected)
                color: Theme.colors.ash
                font.family: fontSet.mono
                font.pixelSize: Theme.footer.text
              }

              Rectangle {
                id: confirmChip
                implicitWidth: confirmLabel.implicitWidth + 12
                implicitHeight: confirmLabel.implicitHeight + 6
                color: Theme.colors.paper
                opacity: root.confirmText !== "" ? 1 : 0

                Behavior on opacity {
                  NumberAnimation { duration: Theme.motion.duration; easing.type: Easing.InOutQuad }
                }

                Text {
                  id: confirmLabel
                  anchors.centerIn: parent
                  text: root.confirmText !== "" ? root.confirmText : " "
                  color: Theme.colors.ink
                  font.family: fontSet.mono
                  font.weight: Theme.fonts.mono.weightBold
                  font.pixelSize: Theme.footer.chip
                }
              }
            }
          }
        }
      }
    }
  }
}
