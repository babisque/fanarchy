.pragma library

// Fanarchy design tokens. Every color, font and size the plugin draws with
// comes from here (specs/00-overview.md, Visual Identity). Fanarchy keeps
// its own palette instead of following the Omarchy theme.

var colors = {
  ink: "#121211",       // backgrounds, text on paper
  inkRaised: "#1B1B1A", // chart area, code blocks
  line: "#2E2D2A",      // borders, grid lines
  paper: "#ECE8DE",     // main text on ink, cards, tape labels
  ash: "#9A968C",       // secondary text, disabled state
  riot: "#E23B2E",      // accent: shapes and large text only
  transparent: "transparent"
}

// `family` is the name the font registers under; `files` are relative to
// plugin/assets/fonts/. Fonts are bundled, never loaded from the network.
var fonts = {
  stencil: {
    family: "Big Shoulders Stencil Display",
    files: ["BigShouldersStencilDisplay.ttf"],
    weight: 900
  },
  mono: {
    family: "JetBrains Mono",
    files: ["JetBrainsMono-Regular.ttf", "JetBrainsMono-Bold.ttf"],
    weight: 400,
    weightBold: 700
  },
  note: {
    family: "Special Elite",
    files: ["SpecialElite-Regular.ttf"],
    weight: 400
  }
}

// Bar button (specs/04-bar-widget.md, section 5).
var bar = {
  height: 26,
  paddingX: 8,
  gap: 6,
  icon: 16,
  border: 1,
  dash: 3,
  number: 13,
  numberHot: 14,
  pressScale: 0.94,
  hoverWash: 0.14
}

var tooltip = {
  width: 340,
  rotation: -1,
  padding: 18,
  offset: 10,
  title: 22,
  value: 13,
  note: 12,
  rowGap: 6,
  columnGap: 18,
  tapeWidth: 180,
  tapeHeight: 46,
  tapeRotation: 2
}

var motion = {
  duration: 200
}
