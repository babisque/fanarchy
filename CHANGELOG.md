# Changelog

All notable changes to Fanarchy are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). Any change to the socket protocol
(`docs/protocol.md`) or the config schema is a major version.

## [Unreleased]

### Changed

- `fanarchyd check-config` takes the path as optional and defaults to
  `/etc/fanarchy/config.json`, like `fanarchyd run`.

## [1.0.1] - 2026-11-23

### Security

- Pump outputs are now protected at the physical level. Before, a fan and a
  pump that used different chip aliases for the same chip (e.g.
  `"board": "it8689"` and `"board2": "it8689_61030507"`) could both claim the
  same `pwmN`, pass validation, and the daemon would drive the pump. Every
  entry, pumps included, is now resolved to its hwmon device and `pwmN`, and
  the daemon refuses to start, `reload` or `set_curve` when a fan lands on a
  pump's output or two entries share an output. Reported in the Omarchy
  plugin marketplace review.
- Defence in depth: the control loop refuses any write to a pump's output,
  whatever the resolved plan says, and so does the in-process restore.
  `fanarchyd restore` skips a fan on any output a pump could be on.

### Changed

- Two aliases in `chips` that resolve to the same chip are rejected. Use one
  alias per chip.
- A pump whose chip is not found is now an error instead of being ignored.

## [1.0.0] - 2026-11-22

First public release.

### Added

- `fanarchyd`: a root daemon that finds `hwmon` chips by name, reads
  temperatures and fan RPM, and applies temperature → speed curves to the
  `pwmN` outputs marked `"role": "fan"`. Pumps (`"role": "pump"`) are never
  written.
- Safety rules: 100 % above `critical_temp_c` (85 °C by default), 100 % for
  fans whose sensor fails, a `min_percent` floor per fan, and every controlled
  fan handed back to the motherboard on stop, crash or kill
  (`fanarchyd restore` as `ExecStopPost`).
- Optional smoothing per fan: `hysteresis_c`, `step_up_percent`,
  `step_down_percent`.
- Optional `rpm_input` per output, for headers whose tachometer is not
  `fanN_input` with the same `N` as `pwmN`.
- JSON Lines socket at `/run/fanarchy.sock` (`root:fanarchy`, `0660`) with
  `status`, `get_config`, `set_curve`, `set_mode`, `reload` and `subscribe`.
- GPU temperatures from `nvidia-smi`.
- `fanarchyctl`: `status`, `config`, `set-curve`, `set-mode`, `reload`, and
  `init`, which detects the hardware and writes a starter config that controls
  nothing until you add fans.
- `fanarchyd check-config` and `fanarchyd status` for read-only checks.
- Omarchy shell plugin `io.github.babisque.fanarchy`: a bar widget with CPU
  temperature and fan state, and a panel with per-fan cards, a draggable
  curve editor and curve / fixed / motherboard modes.
- `PKGBUILD`s for `fanarchy` and `fanarchy-git` (AUR listing coming soon;
  build with `makepkg -si` from `packaging/` until then), a sandboxed systemd
  unit that stays disabled after install, and the `fanarchy` group via
  `sysusers.d`.
- CI: `cargo fmt`, `cargo clippy`, `cargo test` and the plugin model tests.

[1.0.1]: https://github.com/babisque/fanarchy/releases/tag/v1.0.1
[1.0.0]: https://github.com/babisque/fanarchy/releases/tag/v1.0.0
