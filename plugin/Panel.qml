import QtQuick
import qs.Commons
import qs.Ui

// Fanarchy panel: fan list and curve editor.
// Placeholder until Phase 5 builds the real UI. The bar widget toggles it
// with `omarchy-shell shell toggle io.github.babisque.fanarchy`.
Item {
  id: root

  property var shell: null
  property var manifest: null
  property bool opened: false

  function open() { opened = true }
  function close() { opened = false }
  function toggle() { opened = !opened }
}
