# Hardware notes

Measurements and driver behaviour observed on the development machine.
They inform defaults and safety logic; they are not hardcoded anywhere.

## Machine

| Part | Details |
| --- | --- |
| Board | Gigabyte B550M AORUS ELITE |
| Super I/O | ITE IT8689E, community `it87` DKMS driver (frankcrawford/it87) |
| hwmon name | `it8689_61030507` (the suffix comes from the driver and may change between versions; the config uses `it8689`) |
| CPU | Ryzen 5 5600 (`k10temp`, `temp1` = Tctl) |
| GPU | GTX 1660 Super (`nvidia-smi`) |

| Header | Output | Role | Notes |
| --- | --- | --- | --- |
| fan1 | `pwm1` | fan | Case/radiator fans |
| fan2 | `pwm2` | pump | AIO pump. Never written. |
| fan3 | `pwm3` | fan | Case fans. High minimum speed (~780 RPM). |

## IT8689 driver behaviour

- **`pwmN` reads are meaningless while `pwmN_enable=2` (automatic).** The
  value read back does not reflect the duty the chip is applying. `sensors`
  showed "128%" in Phase 1 for this reason. Only trust `pwmN` in manual
  mode (`pwmN_enable=1`).
- **The driver rejects `pwmN` writes in automatic mode with `EBUSY`
  (os error 16).** The daemon's takeover sequence (write `pwmN`, then
  `pwmN_enable=1`, then `pwmN` again) expects this: the first write fails,
  the switch to manual and the second write follow within 0.1 ms. The
  rejection is logged at `debug`.
- Restoring `pwmN_enable=2` hands control back to the BIOS cleanly.

## Phase 2 hardware check (2026-11-21)

PWM scale check with a flat curve on both fans, compared with BIOS auto
control and full speed:

| Fan | Auto (idle) | Flat 50 % | Flat 20 % | Max (100 %) |
| --- | --- | --- | --- | --- |
| fan1 (radiator) | ~1486 RPM | 1155 RPM | 910 RPM ¹ | 1771 RPM |
| fan3 (case) | ~811 RPM | 803 RPM | 781 RPM | 1190 RPM |

¹ Clamped to the radiator's `min_percent` of 25 %, as expected.

- **PWM scale is correct:** 50 % lands between auto and max on both fans.
- **No stalls at 20 %.** fan3 barely drops below ~780 RPM whatever the duty,
  so low curve points on it change little.
- **Real curves under load** (`stress-ng`): Tctl peaked at 71.8 °C. The fans
  ramped up with the temperature and back down after the load stopped.
- **Restore** to `pwmN_enable=2` worked on every stop. `pwm2` was never
  written.
