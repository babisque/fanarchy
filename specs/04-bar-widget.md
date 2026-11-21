# Spec — Phase 4: Bar Widget

## 1. Overview and Goal
* **Purpose:** Create the `io.github.babisque.fanarchy` plugin with a `bar-widget` that shows the CPU temperature and fan state in the Omarchy bar, and opens the panel (Phase 5) on click. Layout and states follow the "Bar widget — states" artboard of the mockups (see `00-overview.md` § Visual Identity).
* **Success Criteria:**
  * `omarchy plugin validate .` passes with no errors.
  * The widget shows the Fanarchy mark and the CPU temperature (e.g. `47°`), refreshed every 2 s through the socket's `subscribe`.
  * On hover, a tooltip shows the GPU temperature and each fan's RPM and %, using the names from the config. The pump shows `AUTO` instead of a %.
  * The widget has three states, as in the mockups (below).
  * Clicking opens the Phase 5 panel.

## 2. Tech Stack and Architecture
* **Core:** QML + JavaScript on the Omarchy shell (Quickshell).
* **Manifest** (`manifest.json` at the repository root, the format from the marketplace development guide):
  ```json
  {
    "schemaVersion": 1,
    "id": "io.github.babisque.fanarchy",
    "name": "Fanarchy",
    "version": "0.1.0",
    "author": "babisque",
    "license": "MIT",
    "description": "Fan curves for Omarchy. No masters, no stock curves.",
    "kinds": ["bar-widget", "panel"],
    "entryPoints": {
      "barWidget": "plugin/BarWidget.qml",
      "panel": "plugin/Panel.qml"
    },
    "barWidget": {
      "displayName": "Fanarchy",
      "category": "system",
      "allowMultiple": false,
      "defaultSection": "right"
    }
  }
  ```
  Check `schemaVersion`, the `panel` entry-point key and the `category` value against `omarchy plugin clone omarchy.clock` and `omarchy plugin validate` before relying on them. If a cloned manifest has `omarchy.clonedFrom`, remove it.
* **Data:** a JS model (`plugin/models/FanarchyClient.js`) connects to `/run/fanarchy.sock`, sends `subscribe` and exposes the latest status. The UI only reads from this model.
* **Reconnect:** if the socket drops, retry every 5 s without blocking the bar.
* **Theme:** colors and fonts come from `plugin/models/theme.js` (tokens in `00-overview.md`).

## 3. Commands
* **Validate:** `omarchy plugin validate .`
* **Enable:** `omarchy plugin enable io.github.babisque.fanarchy --section right`
* **Logs:** run `omarchy-shell` from a terminal (hot reload applies changes on save)

## 4. Project Structure
* `manifest.json`
* `plugin/BarWidget.qml`: visuals only.
* `plugin/models/FanarchyClient.js`: socket, reconnect and state.
* `plugin/models/format.js`: °C, RPM and % formatting.
* `plugin/models/theme.js`: color and font tokens.
* `plugin/assets/logo-mark.svg`: copy of `design/logo-mark.svg`.
* `plugin/assets/tape-2.png`: tape on the tooltip.
* `plugin/assets/fonts/`: bundled fonts and their licenses.

## 5. Code Style and UX Conventions
* **The button:** 26 px tall inside the bar, 8 px horizontal padding, 6 px gap between icon and number. Icon 16 px (`logo-mark.svg`). Number in JetBrains Mono 700, 13 px.
* **States:**

  | State | When | Look |
  | --- | --- | --- |
  | Normal | CPU below `critical_temp_c − 10` | 1 px `paper` border, transparent fill, `paper` text, `47°` |
  | Running hot | CPU at or above `critical_temp_c − 10` | `riot` fill and border, `ink` text and icon, `86° !` at 14 px |
  | Daemon down | socket unreachable | 1 px dashed `ash` border, `ash` text, ring with a slash instead of the fan, `—°` |

* **Tooltip:** `paper` card, ~340 px wide, rotated −1°, with `tape-2.png` across the top edge. Title "FANARCHY" in Big Shoulders Stencil 22 px; a two-column grid (name, value) in JetBrains Mono 13 px; a last line in Special Elite 12 px: "click to open the panel".
* **Daemon down tooltip:** says "Daemon stopped. Fans are on BIOS control." plus `sudo systemctl start fanarchyd`.
* **Never** show "Loading...", empty values or `NaN`. Before the first status, show `—°`.
* **Micro-interactions:** 200 ms ease-in-out on hover and click.
* **Accessibility:** the button has an accessible name ("Fanarchy: CPU 47 degrees").

## 6. Boundaries
* **Always:**
  * Use official widgets (e.g. `omarchy.clock`) as the reference for structure.
  * Follow the protocol in `docs/protocol.md` exactly.
  * Take every color and font from `theme.js`.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before changing the default look of the bar or any widget that is not ours.
* **Never:**
  * NEVER use the `omarchy.` prefix in the plugin ID.
  * NEVER read or write `/sys` from the plugin. Everything goes through the daemon.
  * NEVER load fonts or images from the network.
  * NEVER start a second Quickshell process, add symlinks to the plugin folder or point entry points outside the repository.
