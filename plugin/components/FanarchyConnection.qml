import QtQuick
import Quickshell.Io
import "../models/FanarchyClient.js" as Client

// Owns the subscribed socket to fanarchyd. Every event is handed to
// FanarchyClient.js, which decides the new state; this file only wires
// Quickshell to it. While `active` is false the socket stays closed.
Item {
  id: root

  property bool active: true
  property var clientState: Client.initialState()
  readonly property var view: Client.view(clientState)

  visible: false

  function connect() {
    socket.connected = false
    socket.connected = true
  }

  onActiveChanged: {
    if (active) connect()
    else {
      socket.connected = false
      root.clientState = Client.onDisconnected(root.clientState, "")
    }
  }

  Socket {
    id: socket
    path: Client.SOCKET_PATH

    parser: SplitParser {
      onRead: function(data) {
        var result = Client.onLine(root.clientState, data)
        if (result.log) console.warn(result.log)
        root.clientState = result.state
      }
    }

    onConnectionStateChanged: {
      if (socket.connected) {
        root.clientState = Client.onConnected(root.clientState)
        socket.write(Client.subscribeLine())
        socket.flush()
      } else {
        root.clientState = Client.onDisconnected(root.clientState, "")
      }
    }

    onError: function(error) {
      root.clientState = Client.onDisconnected(root.clientState, Client.socketErrorText(error))
    }
  }

  // Retry while the daemon is unreachable. Connecting is asynchronous, so
  // the shell never waits on it.
  Timer {
    interval: Client.RECONNECT_MS
    repeat: true
    running: root.active && !socket.connected
    onTriggered: root.connect()
  }

  Component.onCompleted: if (root.active) root.connect()
}
