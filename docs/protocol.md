# fanarchy IPC protocol (v1)

How `fanarchyctl` and the Omarchy plugin talk to `fanarchyd`. The Rust
types live in the `protocol/` crate (`fanarchy_protocol`), and this
document describes the same messages.

## Transport

- Unix stream socket at `/run/fanarchy.sock`.
- Owner `root:fanarchy`, mode `0660`. Members of the `fanarchy` group can
  use it without root. If the group does not exist, the daemon falls back
  to `0600` (root only) and logs a warning. The socket is never
  world-writable.
- The group comes from `/usr/lib/sysusers.d/fanarchy.conf`. To join it,
  run `sudo usermod -aG fanarchy $USER` and log in again.
- Clients may stay connected and send any number of requests. Many
  clients can be connected at once.

## Framing

- JSON Lines: one JSON object per line, ending in `\n`, in both directions.
- Each request gets exactly one response line, in order (except
  `subscribe`, see below).
- Blank lines are ignored.
- A line longer than 64 KiB (65536 bytes, newline included) gets an error
  response. The rest of that line is discarded, and the connection stays
  open.
- A malformed line gets an error response, and the connection stays open.

## Responses

```json
{"ok":true,"data":{...}}
{"ok":true}
{"ok":false,"error":"unknown fan id 'cpu'"}
```

- `data` is left out when a request has nothing to return (`set_curve`,
  `set_mode`, `reload`).
- `error` is a short, human-readable message that says what to do. Show
  it to the user as is.

## Requests

Fans are always addressed by their config `id`. Only `apply_setup` names
a chip alias and a `pwmN` number, and only for chips root already
declared in `chips`; no request takes a path. A `pump` output can never
be controlled: requests for a pump are refused with
`fan '<id>' is a pump and cannot be controlled`.

Unknown fields are rejected (`invalid set_curve request: unknown field
'x'`), and so are unknown commands (`unknown cmd 'x' (expected one of:
...)`).

### `status`

```json
{"cmd":"status"}
```

Returns the [status payload](#status-payload).

### `get_config`

```json
{"cmd":"get_config"}
```

Returns the config the daemon is running with, in the same format as
`/etc/fanarchy/config.json` (schema v1). Numbers come back as floats, e.g.
`[[40.0, 30.0], ...]`. Optional fields that are not set are left out.

### `set_curve`

```json
{"cmd":"set_curve","fan":"case","points":[[30,25],[60,50],[80,100]]}
```

- `points` are `[temp_c, percent]` pairs.
- The curve is checked with the same rules as the config file:
  - at least 2 points
  - temperatures 0–120 °C and strictly increasing
  - percentages 0–100 and never decreasing
- If the curve is valid, it is saved to the config file atomically (temp
  file plus rename) and applied right away. Speed changes still follow
  the fan's smoothing settings.
- On any error nothing is changed, neither the running fans nor the file.

`min_percent` and the safety rules still apply on top of the curve:
- a fan never runs below `min_percent`
- above the critical CPU temperature it runs at 100 %
- if a sensor fails it runs at `sensor_fail_percent`

### `set_mode`

```json
{"cmd":"set_mode","fan":"case","mode":"fixed","percent":60}
{"cmd":"set_mode","fan":"case","mode":"curve"}
{"cmd":"set_mode","fan":"case","mode":"auto"}
```

| mode | meaning |
| --- | --- |
| `curve` | follow the fan's curve (the default) |
| `fixed` | run at `percent` %. Required, from the fan's `min_percent` to 100. |
| `auto` | hand the fan to the motherboard (`pwmN_enable = 2`) |

- `percent` is only allowed with `fixed`.
- Safety still applies in every mode. In `fixed` and `auto`, a critical
  CPU or a failed sensor drives the fan exactly as in `curve`. An `auto`
  fan is taken back for that, then handed to the motherboard again once
  it clears.
- The mode is **not saved**. Every fan goes back to `curve` when the
  daemon restarts, on `reload` and on `apply_setup`.

### `reload`

```json
{"cmd":"reload"}
```

Re-reads and validates the config file and applies it, with the same
rules as at startup. Since 1.1.0 the new file may also change which
outputs are controlled (added, removed or moved fans and pumps); they are
re-planned live, as described under [`apply_setup`](#apply_setup). Before
1.1.0 such a file was refused with `controlled outputs changed; restart
the daemon (sudo systemctl restart fanarchyd)`.

On any error the daemon keeps running the previous config.

### `list_outputs`

*Since 1.1.0.*

```json
{"cmd":"list_outputs"}
```

Lists every `pwmN` output on the chips declared in the config's `chips`,
in alias order. Read-only: nothing is written. Readings are from the
daemon's last tick (at most a second old).

```json
{
  "starter_curve": [[40.0, 30.0], [60.0, 50.0], [75.0, 80.0], [85.0, 100.0]],
  "outputs": [
    {
      "chip": "board",
      "pwm": 3,
      "path": "/sys/class/hwmon/hwmon1/pwm3",
      "mode": "manual",
      "percent": 40,
      "rpm": 793,
      "rpm_input": 3,
      "assigned": {"kind": "fan", "id": "case"}
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `starter_curve` | the curve a new fan starts with, before it is raised to the fan's `min_percent`. Defined by the daemon only. |
| `chip` | the chip alias from the config |
| `path` | the `pwmN` file, for display. Valid for this boot only. |
| `mode` | `pwmN_enable`: `manual`, `automatic` or `full` (`0`). `null` when unreadable. |
| `percent` | current duty cycle in %, `null` when unreadable |
| `rpm` | `fanN_input` with the same `N`, `null` when there is none |
| `rpm_input` | the tachometer the daemon reads by default (the same `N`) when `fanN_input` exists, otherwise `null` |
| `assigned` | the config entry on this output: `kind` is `fan`, `pump` or `none`; `id` is `null` for `none` |

A chip that is missing or matches more than one device lists no outputs.

### `apply_setup`

*Since 1.1.0.*

```json
{"cmd":"apply_setup",
 "pumps":[{"id":"pump","label":"AIO pump","chip":"board","pwm":2}],
 "fans":[{"id":"case","label":"Case fans","chip":"board","pwm":3,
          "sensor":"cpu/temp1","min_percent":20,"rpm_input":3,
          "curve":[[40,30],[60,50],[75,80],[85,100]]}]}
```

Replaces the config's `fans` list (fans and pumps) and nothing else.
`chips`, `safety` and `version` are never changed through the socket, and
the request has no fields for them.

- `pumps[]`: `id`, `label`, `chip`, `pwm`.
- `fans[]`: `id`, `label`, `chip`, `pwm`, `sensor`, `min_percent`, plus
  optional `rpm_input` (left out of the file when it equals `pwm`) and
  `curve` (the starter curve, raised to `min_percent`, when left out).
  `hysteresis_c`, `step_up_percent` and `step_down_percent` are kept from
  the fan with the same `id`, if there was one.
- Every pump already in the config must be sent again with the same `id`,
  `chip` and `pwm`. Leaving one out, moving it, or sending a fan with its
  `id` or output is refused with:

  ```
  pump 'pump' can only be removed by editing the config file as root
  ```

- The new config is checked with the same rules as the config file
  (schema, then the hardware: chips, `pwmN`, sensors, one alias per chip,
  no two entries on one physical output, no fan on a pump's output).
- If it is valid, the daemon switches to it without a restart:
  - new fan outputs are taken over with the same sequence as at startup
    (safe duty first, then manual mode), with every safety rule applied;
  - outputs that are no longer fans go back to their original
    `pwmN_enable`, as on shutdown;
  - unchanged outputs keep running and are not written. Their mode goes
    back to `curve`.
- Then the config is saved atomically. If any step fails, every step is
  undone, the previous config keeps running and the file is unchanged.

Returns the new config, like `get_config`.

### `subscribe`

```json
{"cmd":"subscribe"}
```

The daemon replies with a status response right away, then sends one
every 2 seconds until the client disconnects. Each one is a normal
`{"ok":true,"data":<status payload>}` line. Any further lines the client
sends on that connection are ignored. Use a second connection for other
requests.

## Status payload

```json
{
  "fans": [
    {
      "id": "radiator",
      "label": "Radiator fans",
      "role": "fan",
      "sensor": "cpu/temp1",
      "temp_c": 48.25,
      "rpm": 1180,
      "percent": 38,
      "min_percent": 25,
      "mode": "curve",
      "override": "none"
    },
    {
      "id": "pump",
      "label": "AIO pump",
      "role": "pump",
      "sensor": null,
      "temp_c": null,
      "rpm": 2950,
      "percent": null,
      "min_percent": null,
      "mode": null,
      "override": null
    }
  ],
  "cpu_temp_c": 48.25,
  "critical_temp_c": 85.0,
  "gpus": [{"index": 0, "name": "NVIDIA GeForce GTX 1660 SUPER", "celsius": 41.0}],
  "gpu_note": null,
  "chips": [
    {
      "name": "k10temp",
      "temps": [{"index": 1, "label": "Tctl", "celsius": 48.25}],
      "fans": [],
      "pwms": []
    }
  ]
}
```

| field | meaning |
| --- | --- |
| `fans` | every output in the config, pumps included, in config order |
| `fans[].role` | `fan` (controlled) or `pump` (never touched) |
| `fans[].sensor` | `"<chip alias>/tempN"` the fan follows; `null` for pumps |
| `fans[].temp_c` | current reading of `sensor` in °C; `null` if unreadable |
| `fans[].rpm` | `fanN_input` with `N` = the config's `rpm_input`, or the same `N` as the output's `pwmN` when it is not set; `null` if unavailable, `0` if stopped |
| `fans[].percent` | speed the daemon applies, rounded %. `null` for pumps and for `auto` fans while the motherboard drives them. |
| `fans[].min_percent` | lowest speed the daemon will set; `null` for pumps |
| `fans[].mode` | `curve`, `fixed` or `auto`; `null` for pumps |
| `fans[].override` | `none`, `critical` (CPU above `critical_temp_c`, 100 %) or `sensor_failed` (`sensor_fail_percent`); `null` for pumps |
| `cpu_temp_c` | the configured `safety.cpu_sensor` in °C; `null` if unreadable |
| `critical_temp_c` | above this CPU temperature every controlled fan runs at 100 % |
| `gpus` | NVIDIA GPUs from `nvidia-smi`, refreshed every 2 s; empty when there is none |
| `gpu_note` | why GPUs could not be read, or `null` |
| `chips` | every hwmon reading. `pwms[].mode` is the raw `pwmN_enable` value (`0` full speed, `1` manual, `2`+ automatic). |

Temperatures are `null` when a sensor is missing or out of range. Never
show `NaN` for them.

## Stability

Messages and fields are never renamed or removed once published. New
optional fields and new commands may be added, so clients must ignore
fields they do not know.

| Version | Change |
| --- | --- |
| 1.1.0 | added `list_outputs` and `apply_setup`; `reload` applies changes to the controlled outputs |
