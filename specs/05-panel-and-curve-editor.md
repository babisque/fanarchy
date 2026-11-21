# Spec — Phase 5: Panel and Curve Editor

## 1. Overview and Goal
* **Purpose:** Build the plugin's `panel`, where the user sees each fan in detail, edits its curve by dragging points and switches its mode (curve, fixed or motherboard auto).
* **Success Criteria:**
  * The panel opens from the widget and lists one card per fan: name, sensor, temperature, RPM, current % and mode.
  * The pump shows up as a read-only card labeled "Pump: automatic control".
  * The curve editor shows the chart (°C × %) with draggable points and a marker for the current temperature moving along the curve.
  * Releasing a point sends `set_curve`, and the fan responds within 2 s.
  * Points can be added and removed (minimum 2, maximum 8).
  * The user can switch the mode between curve, fixed (with a slider) and auto.
  * Every change shows a subtle confirmation, and every daemon error shows the daemon's message.

## 2. Tech Stack and Architecture
* **Core:** QML + JavaScript, `panel` kind.
* **Chart:** drawn in QML (`Canvas` or `Shape`), with no external libraries.
* **Data:** reuses `FanarchyClient.js` from Phase 4. Edits stay in a local draft and are only sent to the daemon when a point is released or the change is confirmed.
* **Plugin-side validation:** points sorted by temperature, % between the fan's minimum and 100. The daemon validates again.

## 3. Commands
* **Validate:** `omarchy plugin validate plugin/`
* **Logs:** run `omarchy-shell` from a terminal
* **Check the effect:** `fanarchyctl status` or `sensors`

## 4. Project Structure
* `plugin/Panel.qml`: panel layout.
* `plugin/components/FanCard.qml`: one fan's card.
* `plugin/components/CurveEditor.qml`: the chart with draggable points.
* `plugin/models/curve.js`: point rules (sort, clamp, add, remove). Pure functions only.

## 5. Code Style and UX Conventions
* **Visual breathing room:** generous spacing between cards. Nothing cramped.
* **Dragging:** the dragged point grows, shows "60 °C → 45%" next to it and gets highlighted. The cursor changes.
* **Danger zone:** the band above the critical limit is shaded on the chart.
* **Omarchy theme:** colors, fonts and corners from the active theme.
* **No empty states:** without the daemon, the panel shows the state and the command to start it. With no fans configured, it shows how to configure them.
* **No "Loading...":** show the card layout with placeholder values.

## 6. Boundaries
* **Always:**
  * Prevent curves in the editor that drop below the fan's minimum.
  * Show the daemon's error when it rejects a curve, and revert the chart to its previous state.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before adding any external library to the plugin.
* **Never:**
  * NEVER offer pump control in the panel.
  * NEVER send a curve to the daemon on every dragged pixel. Send only on release.
  * NEVER build a generic, AI-template-looking UI.
