# Spec — Phase 9: Showcase and Compatibility List

## 1. Overview and Goal
* **Purpose:** Make the README show what Fanarchy does today (the setup wizard, not only the panel) and start a public list of boards it works on, fed by the "Hardware report" issues.
* **Success Criteria:**
  * The README shows the setup wizard: a screenshot of step 1 ("MARK THE PUMP") and a short demo GIF of the whole setup, both taken on my machine.
  * `preview.png` (used by the marketplace) shows the panel with real fans and curves.
  * `docs/hardware.md` lists tested boards in a table, starting with mine, and the README links to it.
  * The README explains how to add a board: open a "Hardware report" issue.
  * Every image in the README stays under 2 MB, and the GIF under 5 MB.

## 2. Tech Stack and Architecture
* **Screenshots:** `grim` (full output, with a delay) and `magick` to crop.
* **Demo:** `wf-recorder` to record a region (or any existing MP4, such as one from Omarchy's screen recorder), then `ffmpeg` to turn it into a GIF (2-pass palette, 64 colours, no dithering, 12 fps). Default width 720 px; the panel demo uses its native 560 px.
* **Helper:** `scripts/record-demo.sh` wraps the commands above so the media can be redone after UI changes. It records or takes `--from FILE.mp4`, and can `--crop`, `--start`, `--end` and set `--width`. It only records and converts; it never touches the daemon or `/sys`.

## 3. Commands
* **Screenshot with delay:** `sleep 5 && grim -o DP-1 /tmp/full.png`, then `magick /tmp/full.png -crop WxH+X+Y out.png`
* **Record:** `scripts/record-demo.sh docs/assets/setup-demo.gif` (prints how to select the region and stop with Ctrl+C)
* **From an existing recording:** `scripts/record-demo.sh --from rec.mp4 --crop 560x1050+1360+30 --start 0.5 --end 23.5 --width 560 docs/assets/setup-demo.gif`
* **Check sizes:** `du -h docs/assets/* preview.png`

## 4. Project Structure
* `preview.png`: panel with fans and curves (repo root, the marketplace reads it there).
* `docs/assets/setup-step1.png`: wizard step 1.
* `docs/assets/setup-demo.gif`: the whole setup, from START SETUP to "✓ setup applied".
* `docs/hardware.md`: the compatibility table.
* `scripts/record-demo.sh`: recording helper.

## 5. Code Style and Conventions
* **`docs/hardware.md` table columns:** Board, Chip / driver, Module options, Pump, Result, Report (issue link). Result is one of "Works", "Partly works", "Doesn't work".
* **First row (mine):** Gigabyte board with IT8689E, community it87 (DKMS), `ignore_resource_conflict=1`, pump on `pwm2`, Works. Ask me for the exact board model before writing it.
* **README:** a short "Supported hardware" section with one sentence, the link to `docs/hardware.md` and the link to the "Hardware report" issue form. No long tables in the README itself.
* **Images:** real screenshots only, no mockups or edited fake data.

## 6. Boundaries
* **Always:**
  * Keep the README order from Phase 6; the new media goes into the "What it is" and "Install" sections.
* **Ask first:**
  * Before adding a row to `docs/hardware.md` that doesn't come from an issue or from me.
* **Never:**
  * NEVER add a board as "Works" without a report.
  * NEVER commit screenshots that show other windows, notifications or personal data.
