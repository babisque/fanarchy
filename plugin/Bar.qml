import QtQuick
import qs.Commons
import qs.Ui

// Fanarchy bar widget: fan icon + CPU temperature.
// Placeholder until Phase 4 wires it to the daemon through models/.
BarWidget {
  id: root
  moduleName: "babisque.fanarchy"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "󰈐 --°"
    tooltipText: "Fanarchy"
  }
}
