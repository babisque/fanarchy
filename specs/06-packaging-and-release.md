# Spec — Phase 6: Packaging and Release

## 1. Overview and Goal
* **Purpose:** Make Fanarchy installable by any Omarchy user, with clear documentation, and list it on the Omarchy marketplace (plugins.omarchy.org).
* **Success Criteria:**
  * `yay -S fanarchy` installs the daemon, the CLI and the systemd service, and creates the `fanarchy` group.
  * `omarchy plugin add https://github.com/babisque/fanarchy --enable` installs and enables the plugin.
  * On first run without a config, `fanarchyctl init` detects the hardware and writes a starter config **with no controlled outputs**. The user chooses what to control.
  * The README covers installation, the ITE chip issue on Gigabyte boards (community `it87` driver), how to mark the pump and how to uninstall.
  * GitHub CI runs `cargo fmt --check`, `cargo clippy` and `cargo test` on every pull request.
  * v1.0.0 ships with a tag, a GitHub release, an AUR package and a marketplace listing.

## 2. Tech Stack and Architecture
* **Service:** `packaging/fanarchyd.service` (systemd), with `Restart=on-failure`, `ExecStopPost=/usr/bin/fanarchyd restore` and systemd hardening (`ProtectSystem`, `ProtectHome`, `ReadWritePaths` limited to `/sys/class/hwmon`, `/etc/fanarchy` and `/run`).
* **Package:** `PKGBUILD` for the AUR (`fanarchy` for releases and `fanarchy-git` for development).
* **Group:** created through `sysusers.d`.
* **CI:** GitHub Actions.
* **License:** MIT.
* **Marketplace:** the repository must be public, with a valid `manifest.json` at the root, a README and a LICENSE. Submission is the marketplace's "Submit a plugin" GitHub issue form (repository link, category and tags). Automated checks run on the current commit before a maintainer approves.

## 3. Commands
* **Release build:** `cargo build --release`
* **Test the package locally:** `makepkg -si` (inside `packaging/`)
* **Check the PKGBUILD:** `namcap PKGBUILD`
* **Validate the plugin:** `omarchy plugin validate .`

## 4. Project Structure
* `packaging/PKGBUILD`, `packaging/fanarchyd.service`, `packaging/fanarchy.sysusers`
* `.github/workflows/ci.yml`
* `README.md`
* `preview.png`: at the root, for the marketplace card.
* `CHANGELOG.md`
* `LICENSE`, plus the font licenses in `plugin/assets/fonts/`.

## 5. README and Marketplace Presentation
* **Hero:** `docs/assets/banner.jpg` at the top, full width. The README hero artboard in the mockups shows the intended look: the ransom-letter wordmark on the empty left side, the tagline "No masters. No stock curves." and `yay -S fanarchy`. If the wordmark can't be laid out in Markdown, put the title right under the image.
* **Logo:** `design/logo-animated.gif` may appear once, small, in the "What it is" section. Never above the fold if it pushes the install steps down.
* **GitHub social preview:** upload `docs/assets/social-preview.jpg` in the repository settings (manual step, I do it).
* **`preview.png`:** a **real screenshot** of the widget and the open panel running on my machine. Never an AI-generated image or a mockup render.
* **README order:** what it is (one sentence + the screenshot), install in 3 commands, the privilege boundary (a root daemon, what it can write, what it never touches), configuration (marking the pump first), known issues (`it87`, `ignore_resource_conflict=1` and its risk), uninstall.
* **Versioning:** semantic versioning. Any change to the protocol or the config schema is a major version. The CHANGELOG is updated on every release.

## 6. Boundaries
* **Always:**
  * Leave the service disabled after installation. The user enables it after configuring.
  * Document the risk of `ignore_resource_conflict=1` in the README.
  * Document every external dependency, setup step, privilege boundary and service in the README, as the marketplace guide asks.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before publishing anything (tag, release, AUR, marketplace issue).
* **Never:**
  * NEVER install or configure kernel modules (`it87`) through the package. That stays documented in the README as a manual step.
  * NEVER enable the service automatically on install.
  * NEVER expose tokens, AUR keys or secrets in the repository or CI.
  * NEVER ship `design/` files (reference images, the GIF) inside the plugin's runtime assets.
