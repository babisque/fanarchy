# Spec — Phase 5: Panel and Curve Editor

## 1. Overview and Goal
* **Purpose:** Build the plugin's `panel`, where the user sees each fan in detail, edits its curve by dragging points and switches its mode (curve, fixed or motherboard auto). Layout follows the "Panel + curve editor" and "Offline + empty states" artboards of the mockups (see `00-overview.md` § Visual Identity).
* **Success Criteria:**
  * The panel opens from the widget and lists one card per fan: name, header and sensor, RPM, current % and mode.
  * The pump shows up as a read-only card labeled "Pump · automatic control, always", with a "HANDS OFF" tape.
  * The curve editor shows the chart (°C × %) with draggable points, the danger zone and a marker for the current temperature.
  * Releasing a point sends `set_curve`, and the fan responds within 2 s.
  * Points can be added and removed (minimum 2, maximum 8).
  * The user can switch the mode between curve, fixed (with a slider) and auto.
  * Every change shows a subtle confirmation, and every daemon error shows the daemon's message.

## 2. Tech Stack and Architecture
* **Core:** QML + JavaScript, `panel` kind.
* **Chart:** drawn in QML (`Canvas` or `Shape`), with no external libraries.
* **Data:** reuses `FanarchyClient.js` from Phase 4. Edits stay in a local draft and are only sent to the daemon when a point is released or the change is confirmed.
* **Plugin-side validation:** points sorted by temperature, % between the fan's minimum and 100. The daemon validates again.
* **Theme:** colors and fonts from `plugin/models/theme.js`.

## 3. Commands
* **Validate:** `omarchy plugin validate .`
* **Logs:** run `omarchy-shell` from a terminal
* **Check the effect:** `fanarchyctl status` or `sensors`

## 4. Project Structure
* `plugin/Panel.qml`: panel layout.
* `plugin/components/FanCard.qml`: one fan's card.
* `plugin/components/PumpCard.qml`: the read-only pump card.
* `plugin/components/CurveEditor.qml`: the chart with draggable points.
* `plugin/components/EmptyState.qml`: daemon stopped and no-fans states.
* `plugin/models/curve.js`: point rules (sort, clamp, add, remove). Pure functions only.
* `plugin/assets/`: `paper-grain.png`, `tape-1.png`, `tape-3.png`, `logo.svg` (copy of `design/logo.svg`).

## 5. Code Style and UX Conventions
* **Panel frame:** 560 px wide, `ink` background with `paper-grain.png` tiled at 512 × 512 px, a 3 px `riot` left border, 32 px top/bottom and 28 px side padding, 24 px between sections.
* **Header:** `logo.svg` at 40 px, "FANARCHY" in Big Shoulders Stencil 900 36 px, and on the right "CPU **47°** · GPU **41°**" in JetBrains Mono 14 px.
* **Fan cards:** full-width buttons, at least 64 px tall, 14 × 16 px padding, 10 px apart.
  * Left: name in Big Shoulders Stencil 24 px (uppercase) and `fan1 · pwm1 · <sensor>` in 12 px `ash`.
  * Right: RPM in 18 px bold and `34% · curve` in 12 px `ash`.
  * Selected: `paper` fill and `ink` text. Not selected: transparent with a 2 px `line` border.
* **Pump card:** 1 px dashed `ash` border, all text in `ash`, `tape-3.png` with "HANDS OFF" (11 px bold, tracked) taped across the top-right corner, rotated 3°. Not clickable.
* **Curve editor:** title "CURVE — <FAN NAME>" in Big Shoulders Stencil 26 px, with the sensor name on the right. Chart on `inkRaised` with a 1 px `line` border.
  * X axis 20–100 °C, Y axis 0–100 %, grid lines in `line`, axis labels in 11 px `ash`.
  * Curve: 3 px `paper` line. Points: 7 px circles, `ink` fill, 3 px `paper` stroke.
  * Dragged point: grows to 11 px, `paper` fill with a `riot` stroke, and shows a `paper` label like "65° → 55%". The cursor changes.
  * Current temperature: a 2 px dashed `riot` vertical line plus a 6 px `riot` dot where it meets the curve.
  * Danger zone: everything above `critical_temp_c` hatched with `riot` diagonal stripes at 45% opacity, labeled "≥85° FULL".
  * Under the chart, in Special Elite 13 px `ash`: "Drag a point. Release to apply. Red line = right now."
* **Mode selector:** three equal buttons, at least 44 px tall: "CURVE", "FIXED", "BIOS". Selected: `riot` fill, `ink` text. Others: transparent with a 2 px `line` border.
* **Footer:** a dashed `line` divider, "daemon online · /run/fanarchy.sock" in 12 px `ash` on the left, and the confirmation ("✓ curve applied") as a small `paper` label on the right.
* **Empty states** (`EmptyState.qml`), each with a tape on the top edge, the logo at 72 px and 50% opacity, a Big Shoulders Stencil 40 px headline, a Special Elite 15 px line, the command in a dashed code box and one button:
  * Daemon stopped: "THE DAEMON STOPPED LISTENING." / "Your fans are safe: the motherboard took control back." / `sudo systemctl start fanarchyd` / "COPY COMMAND".
  * No fans configured: "NOTHING TO REBEL AGAINST. YET." / "No fans are under your control. Scan the hardware, name your fans and mark the pump." / `fanarchyctl init` / "START SETUP".
* **No "Loading...":** show the card layout with `—` values.
* **Transitions:** 200 ms ease-in-out on cards, buttons and mode changes.

## 6. Boundaries
* **Always:**
  * Prevent curves in the editor that drop below the fan's minimum.
  * Show the daemon's error when it rejects a curve, and revert the chart to its previous state.
  * Take every color and font from `theme.js`.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before adding any external library to the plugin.
* **Never:**
  * NEVER offer pump control in the panel.
  * NEVER send a curve to the daemon on every dragged pixel. Send only on release.
  * NEVER load fonts or images from the network.
  * NEVER build a generic, AI-template-looking UI.
