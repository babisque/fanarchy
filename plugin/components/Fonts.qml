import QtQuick
import "../models/theme.js" as Theme

// Loads the bundled fonts once and exposes their family names. Falls back
// to the token's family name until a file has loaded.
Item {
  visible: false

  readonly property string stencil: stencilFont.name !== "" ? stencilFont.name : Theme.fonts.stencil.family
  readonly property string mono: monoRegular.name !== "" ? monoRegular.name : Theme.fonts.mono.family
  readonly property string note: noteFont.name !== "" ? noteFont.name : Theme.fonts.note.family

  FontLoader { id: stencilFont; source: Qt.resolvedUrl("../assets/fonts/" + Theme.fonts.stencil.files[0]) }
  FontLoader { id: monoRegular; source: Qt.resolvedUrl("../assets/fonts/" + Theme.fonts.mono.files[0]) }
  FontLoader { source: Qt.resolvedUrl("../assets/fonts/" + Theme.fonts.mono.files[1]) }
  FontLoader { id: noteFont; source: Qt.resolvedUrl("../assets/fonts/" + Theme.fonts.note.files[0]) }
}
