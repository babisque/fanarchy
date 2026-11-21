# Spec — Phase 4: Bar Widget

## 1. Overview and Goal
* **Purpose:** Create the `babisque.fanarchy` plugin with a `bar-widget` that shows the CPU temperature and fan state in the Omarchy bar, and opens the panel (Phase 5) on click.
* **Success Criteria:**
  * `omarchy plugin validate plugin/` passes with no errors.
  * The widget shows a fan icon and the CPU temperature (e.g. `42°`), refreshed every 2 s through the socket's `subscribe`.
  * On hover, a tooltip shows the GPU temperature and the RPM of each fan, using the names from the config.
  * The icon has three states: normal, warning (near the critical limit) and "daemon stopped".
  * Clicking opens the Phase 5 panel.
  * The widget looks right on every Omarchy theme.

## 2. Tech Stack and Architecture
* **Core:** QML + JavaScript on the Omarchy shell (Quickshell).
* **Manifest:** `id: babisque.fanarchy`, `kinds: ["bar-widget", "panel"]`, `entryPoints` pointing to `Bar.qml` and `Panel.qml`, `defaultSection: right`.
* **Data:** a JS model (`models/FanarchyClient.js`) connects to `/run/fanarchy.sock`, sends `subscribe` and exposes the latest status. The UI only reads from this model.
* **Reconnect:** if the socket drops, retry every 5 s without blocking the bar.

## 3. Commands
* **Validate:** `omarchy plugin validate plugin/`
* **Enable:** `omarchy plugin enable babisque.fanarchy --section right`
* **Logs:** run `omarchy-shell` from a terminal (hot reload applies changes on save)

## 4. Project Structure
* `plugin/manifest.json`
* `plugin/Bar.qml`: visuals only.
* `plugin/models/FanarchyClient.js`: socket, reconnect and state.
* `plugin/models/format.js`: °C, RPM and % formatting.

## 5. Code Style and UX Conventions
* **Native to Omarchy:** use the active shell theme's colors, fonts and spacing. No hardcoded colors.
* **Compact:** icon + number in the bar. Details go in the tooltip and the panel.
* **Clear states:** a stopped daemon shows a dimmed icon plus a tooltip with `sudo systemctl start fanarchyd`. Never show "Loading...", empty values or `NaN`.
* **Micro-interactions:** smooth hover and click transitions, matching the pace of Omarchy's official widgets.

## 6. Boundaries
* **Always:**
  * Use official widgets (e.g. `omarchy.clock`) as the reference for structure and style.
  * Follow the protocol in `docs/protocol.md` exactly.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before changing the default look of the bar or any widget that is not ours.
* **Never:**
  * NEVER use the `omarchy.` prefix in the plugin ID.
  * NEVER read or write `/sys` from the plugin. Everything goes through the daemon.
  * NEVER build a generic, AI-template-looking UI. The widget must look official to Omarchy.
