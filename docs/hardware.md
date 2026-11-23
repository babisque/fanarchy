# Supported hardware

Boards Fanarchy has been tested on, as reported by the people who run it.

**How to add your board:** open a [Hardware report](https://github.com/babisque/fanarchy/issues/new?template=hardware_report.yml) issue. Every report helps, including "it just works". A board is added here only from a report.

| Board | Chip / driver | Module options | Pump | Result | Report |
| --- | --- | --- | --- | --- | --- |
| Gigabyte B550M AORUS ELITE | ITE IT8689E, community it87 (DKMS) | `ignore_resource_conflict=1` | `pwm2` | Works | Maintainer's machine |

**Result:**

- **Works**: fans follow their curves, and the pump is left alone.
- **Partly works**: some outputs or readings are wrong.
- **Doesn't work**: no PWM outputs, or the daemon refuses to start.

**Module options** are the driver options that had to be set. For `ignore_resource_conflict=1` and its risk, see [Known issues](../README.md#gigabyte-boards-with-ite-chips-it87) in the README.
