import QtQuick
import Quickshell.Io
import "../models/FanarchyClient.js" as Client

// Request connection for the panel (get_config, set_curve, set_mode). The
// subscribed connection ignores requests, so this is a second socket.
// Replies arrive in order; FanarchyClient.settle pairs each with its request.
Item {
  id: root

  property bool active: true
  property var pending: []
  readonly property bool connected: socket.connected

  // Emitted once per request: `ok`, the reply's `data` or the daemon's
  // `error` message, shown to the user as is.
  signal replied(var request, bool ok, var data, string error)
  signal ready()

  visible: false

  function send(request) {
    if (!socket.connected) {
      root.replied(request, false, null, "the daemon is not reachable")
      return
    }
    root.pending = Client.enqueue(root.pending, request)
    socket.write(Client.requestLine(request))
    socket.flush()
  }

  function failPending(reason) {
    var dropped = root.pending
    root.pending = []
    for (var i = 0; i < dropped.length; i++) root.replied(dropped[i], false, null, reason)
  }

  function connect() {
    socket.connected = false
    socket.connected = true
  }

  onActiveChanged: {
    if (active) connect()
    else {
      socket.connected = false
      failPending("the panel was closed")
    }
  }

  Socket {
    id: socket
    path: Client.SOCKET_PATH

    parser: SplitParser {
      onRead: function(data) {
        var result = Client.settle(root.pending, data)
        root.pending = result.queue
        if (result.log) console.warn(result.log)
        if (result.request) root.replied(result.request, result.ok, result.data, result.error || "")
      }
    }

    onConnectionStateChanged: {
      if (socket.connected) root.ready()
      else root.failPending("the daemon disconnected")
    }

    onError: function(error) {
      root.failPending("the daemon disconnected (" + Client.socketErrorText(error) + ")")
    }
  }

  Timer {
    interval: Client.RECONNECT_MS
    repeat: true
    running: root.active && !socket.connected
    onTriggered: root.connect()
  }

  Component.onCompleted: if (root.active) root.connect()
}
