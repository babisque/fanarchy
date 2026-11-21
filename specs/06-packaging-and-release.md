# Spec — Phase 6: Packaging and Release

## 1. Overview and Goal
* **Purpose:** Make Fanarchy installable by any Omarchy user, with clear documentation, and release it to the community.
* **Success Criteria:**
  * `yay -S fanarchy` installs the daemon, the CLI and the systemd service, and creates the `fanarchy` group.
  * `omarchy plugin add https://github.com/babisque/fanarchy --enable` installs and enables the plugin.
  * On first run without a config, `fanarchyctl init` detects the hardware and writes a starter config **with no controlled outputs**. The user chooses what to control.
  * The README covers installation, the ITE chip issue on Gigabyte boards (community `it87` driver), how to mark the pump and how to uninstall.
  * GitHub CI runs `cargo fmt --check`, `cargo clippy` and `cargo test` on every pull request.
  * v1.0.0 ships with a tag, a GitHub release and an AUR package.

## 2. Tech Stack and Architecture
* **Service:** `packaging/fanarchyd.service` (systemd), with `Restart=on-failure` and systemd hardening (`ProtectSystem`, `ProtectHome`, `ReadWritePaths` limited to `/sys/class/hwmon`, `/etc/fanarchy` and `/run`).
* **Package:** `PKGBUILD` for the AUR (`fanarchy` for releases and `fanarchy-git` for development).
* **Group:** created through `sysusers.d`.
* **CI:** GitHub Actions.
* **License:** MIT.

## 3. Commands
* **Release build:** `cargo build --release`
* **Test the package locally:** `makepkg -si` (inside `packaging/`)
* **Check the PKGBUILD:** `namcap PKGBUILD`

## 4. Project Structure
* `packaging/PKGBUILD`, `packaging/fanarchyd.service`, `packaging/fanarchy.sysusers`
* `.github/workflows/ci.yml`
* `README.md`, with screenshots of the widget and the panel.
* `CHANGELOG.md`
* `LICENSE`

## 5. Code Style and Conventions
* A direct README: what it is, a screenshot, install in 3 commands, how to configure, known issues.
* Semantic versioning. Any change to the protocol or the config schema is a major version.
* The CHANGELOG is updated on every release.

## 6. Boundaries
* **Always:**
  * Leave the service disabled after installation. The user enables it after configuring.
  * Document the risk of `ignore_resource_conflict=1` in the README.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before publishing anything (tag, release, AUR, plugin registry).
* **Never:**
  * NEVER install or configure kernel modules (`it87`) through the package. That stays documented in the README as a manual step.
  * NEVER enable the service automatically on install.
  * NEVER expose tokens, AUR keys or secrets in the repository or CI.
