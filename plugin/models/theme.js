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

// Panel (specs/05-panel-and-curve-editor.md, section 5).
var panel = {
  width: 560,
  edge: 3,              // riot border on the left
  padTop: 32,
  padSide: 28,
  section: 24,
  grain: 512,
  logo: 40,
  title: 36,
  temps: 14,
  scrollBar: 4
}

var card = {
  minHeight: 64,
  padY: 14,
  padX: 16,
  gap: 10,
  border: 2,
  name: 24,
  meta: 12,
  rpm: 18,
  tapeWidth: 120,
  tapeHeight: 30,
  tapeRotation: 3,
  tapeText: 11,
  tapeTracking: 1.5
}

var editor = {
  title: 26,
  sensor: 12,
  chartHeight: 240,
  axis: 11,
  axisGutter: 30,       // room for the % labels on the left
  axisFooter: 20,       // room for the °C labels below
  curveWidth: 3,
  point: 7,
  pointStroke: 3,
  pointDragged: 11,
  hitArea: 26,
  markerWidth: 2,
  markerDot: 6,
  dash: 4,
  hatchGap: 9,
  hatchOpacity: 0.45,
  label: 11,
  note: 13,
  button: 32,
  minLineOpacity: 0.8
}

var modes = {
  height: 44,
  gap: 8,
  border: 2,
  text: 20,
  sliderHeight: 28,
  track: 4,
  handle: 18
}

var footer = {
  text: 12,
  chip: 12,
  confirmMs: 2500
}

var empty = {
  logo: 72,
  logoOpacity: 0.5,
  headline: 40,
  line: 15,
  code: 13,
  button: 44,
  copiedMs: 1800
}
