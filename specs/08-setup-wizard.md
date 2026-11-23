# Spec — Phase 8: Setup Wizard in the Panel

## 1. Overview and Goal
* **Purpose:** Replace the hardest part of the install, hand-editing `/etc/fanarchy/config.json`, with a wizard in the panel. The user sees every PWM output the daemon found, marks the pump first, then picks, names and configures the fans to control. The daemon validates and applies the result without a restart.
* **Starting point:** after `sudo fanarchyctl init`, the config has `chips`, `safety` and `"fans": []`. The panel shows the "NOTHING TO REBEL AGAINST. YET." empty state with a "START SETUP" button that today only shows a command.
* **Success Criteria:**
  * "START SETUP" opens the wizard. A "SETUP" button in the panel footer reopens it later, pre-filled with the current config.
  * Step 1 lists every PWM output on the chips declared in the config, with live RPM, and the user marks the pump(s) or confirms there is no pump. Step 2 cannot be reached before this choice.
  * Step 2 lets the user turn control on per output and set its name, sensor, minimum % and, when needed, its tachometer (`rpm_input`).
  * Step 3 shows a review with a starter curve per fan. "APPLY" sends the setup to the daemon, which validates it with the same rules as the config file, saves it atomically and applies it within 2 s.
  * Newly added fans are taken over safely, and removed fans are handed back to the BIOS, without restarting the service.
  * Any rejection shows the daemon's message, and nothing changes.
  * `fanarchyctl outputs` lists the same outputs in a table, for terminal users.
  * Tested end to end on my machine: start from `fans: []`, mark `pwm2` as the pump, control `pwm1` and `pwm3`, and end with the same behaviour as my current config.

## 2. Tech Stack and Architecture
* **Daemon:** Rust, reusing `config::resolve()` (physical output identity), `validate`, `save_atomic` and the existing takeover and restore code.
* **Plugin:** QML + JavaScript, a new view inside the existing panel (not a second window). Logic in `plugin/models/setup.js` as pure functions.
* **Protocol (additive, version 1.1.0; nothing existing is renamed or removed):**
  * `{"cmd":"list_outputs"}` → one entry per PWM output on the chips declared in `chips`:
    `{"chip":"board","pwm":3,"path":"/sys/class/hwmon/hwmon1/pwm3","mode":"manual|automatic|full","percent":40,"rpm":793,"rpm_input":3,"assigned":{"kind":"fan|pump|none","id":"case"}}`.
    `rpm_input` is the tachometer the daemon would read by default (same number as the PWM) when that `fanN_input` exists, otherwise `null`.
  * `{"cmd":"apply_setup","pumps":[{"id":"pump","label":"AIO pump","chip":"board","pwm":2}],"fans":[{"id":"case","label":"Case fans","chip":"board","pwm":3,"sensor":"cpu/temp1","min_percent":20,"rpm_input":3,"curve":[[40,30],[60,50],[75,80],[85,100]]}]}`
    → replaces the `fans` list of the config (fans and pumps) and nothing else. `chips` and `safety` are never changed through this message. Response: the new config, like `get_config`.
* **Runtime re-plan:** today `reload` applies changes but refuses new outputs. `apply_setup` (and `reload`, for consistency) builds a new plan and diffs it against the running one:
  * outputs that are no longer fans → restored to their original `pwmN_enable` (the same restore path as shutdown);
  * new fan outputs → taken over with the safe-PWM-first sequence used at startup;
  * unchanged outputs → keep running, no write.
  * If any step fails, the daemon restores the previous plan and returns an error.
* **Starter curve:** `[[40,30],[60,50],[75,80],[85,100]]`, clamped to the fan's `min_percent`. Defined once in the daemon and sent to the plugin with `list_outputs` (`"starter_curve"` field at the top level), never duplicated in QML.
* **IDs:** the plugin proposes an id from the label (`"Case fans"` → `case-fans`). Lowercase letters, digits and `-`, unique. The user can edit it.

## 3. Commands
* **Test:** `cargo test`, `node --test plugin/tests/`
* **Validate plugin:** `omarchy plugin validate .`
* **List outputs:** `cargo run --bin fanarchyctl -- outputs`
* **Manual socket test:** `echo '{"cmd":"list_outputs"}' | socat - UNIX-CONNECT:/run/fanarchy.sock`
* **Reload the plugin after changes:** `omarchy restart shell` (a plugin rescan keeps the old QML cached)

## 4. Project Structure
* `protocol/src/`: the two new messages and their response types.
* `daemon/src/setup.rs`: `apply_setup` rules (pure functions where possible).
* `daemon/src/control.rs`: plan diff and runtime re-plan.
* `cli/src/main.rs`: `fanarchyctl outputs`.
* `docs/protocol.md`: both messages documented, with the version they appeared in.
* `plugin/components/SetupWizard.qml`: the three steps.
* `plugin/components/OutputRow.qml`: one output (name, path, live RPM, assignment).
* `plugin/models/setup.js`: id slugs, draft validation, building the `apply_setup` payload.
* `plugin/tests/setup.test.mjs`: tests for `setup.js`.

## 5. Code Style and UX Conventions
No mockup exists for this screen yet. Follow the panel frame, tokens and fonts of Phase 5 and `00-overview.md` § Visual Identity.
* **Step header:** "STEP 1 / 3" in JetBrains Mono 12 px `ash`, then the title in Big Shoulders Stencil 32 px: "MARK THE PUMP", "PICK YOUR FANS", "CHECK AND APPLY".
* **Step 1:** one `OutputRow` per output: `pwm2 · board` in Big Shoulders Stencil 22 px, path in 12 px `ash`, live RPM in JetBrains Mono 18 px bold on the right, and a "PUMP" toggle. A marked pump row gets the dashed `ash` border and the `tape-3.png` "HANDS OFF" tape from the pump card. Under the list, in Special Elite 13 px `ash`: "A pump that stops can cook your CPU. Mark it first. Fanarchy will never touch it." Pumps already in the config are locked (toggle disabled) with "to change this, edit the config as root" in 12 px `ash`. Below the list, a secondary button "I HAVE NO PUMP". "NEXT" stays disabled until a pump is marked or "I HAVE NO PUMP" is chosen.
* **Step 2:** pumps are shown read-only at the top. Every other output has a "CONTROL" toggle; when it's on, the row expands to: label (text field), id (prefilled from the label, editable), sensor (dropdown from `status`, default `cpu/temp1`), minimum % (slider 10–60, default 20) and, under "ADVANCED", the tachometer (`rpm_input`) dropdown. A row whose output reads 0 RPM shows "no tachometer reading. Is this header empty, or does its RPM come from another fanN?" in 12 px `ash`.
* **Step 3:** a summary card per fan (name, `pwm3 · cpu/temp1 · min 20%`) and a small read-only preview of its starter curve, plus the pumps in the dashed style. Under the list: "You can change every curve after this. Fans you didn't pick stay with the BIOS." Main button "APPLY" with a `riot` fill.
* **Navigation:** "BACK" and "NEXT"/"APPLY" at the bottom, at least 44 px tall. Closing the panel discards the draft and asks nothing.
* **Errors:** the daemon's message in a `riot`-bordered box above the buttons. The wizard stays on step 3 with the draft intact.
* **Success:** the panel goes back to the fan list, with "✓ setup applied" in the footer.
* **Copy:** short and direct, same voice as the empty states. No emoji.

## 6. Boundaries
* **Always:**
  * Run `apply_setup` through the same `resolve()` as the config file: physical output identity, no duplicate outputs, no fan on a pump's output, no two aliases for one chip.
  * Accept only outputs on chips already declared in `chips` (chosen by root through `init` or by editing the file).
  * Save atomically and keep the previous config active if anything fails.
  * Keep every safety rule of Phase 2 during and after the re-plan (minimum floor, 100% on sensor failure or critical temperature).
  * Show the pump step first, every time the wizard opens.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before changing the order of the steps or adding a fourth one.
  * Before adding an "identify" feature (spinning an output to find which header it drives). It would write to outputs that aren't in the config yet, which goes against `00-overview.md` § Boundaries.
* **Never:**
  * NEVER remove a pump, or turn a pump into a fan, through the socket. Only root can do that, by editing `/etc/fanarchy/config.json`. `apply_setup` must reject it with "pump 'pump' can only be removed by editing the config file as root".
  * NEVER change `chips` or `safety` through the socket.
  * NEVER write to any output during the wizard before "APPLY". Steps 1 and 2 are read-only.
  * NEVER restart the service or ask for root from the plugin.
  * NEVER rename or remove existing protocol messages or fields.
