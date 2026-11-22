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
  // Request and data travel as JSON text (Client.encodeReply): a `var`
  // argument would turn their arrays into Qt sequences on the way.
  signal replied(string requestJson, bool ok, string dataJson, string error)

  function reply(request, ok, data, error) {
    var encoded = Client.encodeReply(request, data)
    root.replied(encoded.requestJson, ok, encoded.dataJson, error || "")
  }
  signal ready()

  visible: false

  function send(request) {
    if (!socket.connected) {
      root.reply(request, false, null, "the daemon is not reachable")
      return
    }
    root.pending = Client.enqueue(root.pending, request)
    socket.write(Client.requestLine(request))
    socket.flush()
  }

  function failPending(reason) {
    var dropped = root.pending
    root.pending = []
    for (var i = 0; i < dropped.length; i++) root.reply(dropped[i], false, null, reason)
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
        if (result.request) root.reply(result.request, result.ok, result.data, result.error)
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
