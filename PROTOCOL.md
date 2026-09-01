# OBSBOT Tiny 3 Lite — Linux control protocol notes

Everything in this document was verified against a real OBSBOT Tiny 3 Lite
(USB `3564:ff04`, firmware `bcdDevice 0510`) unless marked otherwise.
Dead ends are documented too — they save the next person hours.

## Device identification

| Field | Value |
|---|---|
| USB ID | `3564:ff04` (Remo Tech Co., Ltd.) |
| Product string | `OBSBOT Tiny 3 Lite` |
| bcdDevice (fw rev) | `0510` |
| Device class | `EF` (miscellaneous / interface association) |
| Stable video node | `/dev/v4l/by-id/usb-Remo_Tech_Co.__Ltd._OBSBOT_Tiny_3_Lite-video-index0` |

Numeric `/dev/videoN` shifts on re-enumeration — never hardcode it.

## USB interface topology

Parsed from cached sysfs descriptors (`/sys/bus/usb/devices/<port>/descriptors`),
which does **not** open or wake the device:

| Interface | Class | Driver | Role |
|---|---|---|---|
| 0 | `0e/01` video control | `uvcvideo` | UVC control (camera terminal, processing unit, **extension unit**) |
| 1 | `0e/02` video streaming | `uvcvideo` | Video (bulk EP 0x81) |
| 2 | `01/01` audio control | `snd-usb-audio` | Mic control |
| 3 | `01/02` audio streaming | `snd-usb-audio` | Mic (iso EP 0x82) |
| 4 | `02/02/01` CDC ACM control | `cdc_acm` | → `/dev/ttyACM0` (int EP 0x83) |
| 5 | `0a/00` CDC data | `cdc_acm` | Data half of the ACM port (bulk EP 0x85 in / 0x04 out) |

Interfaces 4+5 form **one** serial port. CDC descriptors: header `1001`
(CDC 1.10), call mgmt `0005`, ACM capabilities `02` (line coding only),
union `0405`. This ACM channel is the suspected proprietary transport used
by OBSBOT Center.

## UVC extension unit (interface 0)

```
bUnitID          = 2
guidExtensionCode= 9a1e7291-6843-4683-6d92-39bc7906ee49   (little-endian UUID form)
bNumControls     = 19
bmControls       = ff ff 3f 00   → control bits 0..21 set (selectors 1..22 candidates)
```

Status: discovered from descriptors; selector semantics not yet probed.
This XU is the prime suspect for sleep/wake, AI tracking, and gimbal
preset commands (Tiny4Linux drives the Tiny 2 via a UVC XU).

## Standard UVC controls (verified via `v4l2-ctl --list-ctrls-menus`)

User controls:

| Control | Range | Default | Notes |
|---|---|---|---|
| `brightness` | 0–100 | 50 | |
| `contrast` | 0–100 | 50 | |
| `saturation` | 0–100 | 50 | |
| `hue` | 0–100 | 50 | |
| `white_balance_automatic` | bool | 1 | see WB quirk below |
| `red_balance` / `blue_balance` | 0–255 | 127 | **danger — see WB quirk** |
| `gain` | 1–64 | 1 | |
| `power_line_frequency` | menu 0–2 | 3(!) | 0=off 1=50Hz 2=60Hz |
| `white_balance_temperature` | 2000–10000 step 100 | 5000 | |
| `sharpness` | 0–100 | 50 | |
| `backlight_compensation` | 0–18 | 9 | |

Camera controls:

| Control | Range | Default | Notes |
|---|---|---|---|
| `auto_exposure` | menu 0/1/3 | 0 | 0=Auto 1=Manual 3=Aperture Priority |
| `exposure_time_absolute` | 1–2500 | 330 | inactive while auto |
| `pan_absolute` | ±468000 step 3600 | 0 | ±130° in 1/3600° units |
| `tilt_absolute` | ±324000 step 3600 | 0 | ±90° |
| `focus_absolute` | 0–100 | 0 | inactive while auto-focus on |
| `focus_automatic_continuous` | bool | 1 | |
| `zoom_absolute` | 0–100 | 0 | |
| `zoom_continuous` | -100–100 | 100 | readback can return out-of-range garbage (245 observed) |
| `pan_speed` | -80–80 | 10 | |
| `tilt_speed` | -120–120 | 20 | |

## Firmware quirks (all verified the hard way)

### 1. Last-written-control-wins white balance

The firmware treats whichever WB control was written **most recently** as the
active WB source. To set a manual color temperature:

```
white_balance_automatic=0   # first
(brief pause)
white_balance_temperature=N # LAST — write nothing after it
```

Writing `red_balance`/`blue_balance` *after* temperature makes those values
literal channel gains (127/127 → half red, half blue, double green → saturated
neon-green image). **Control readbacks look correct while the image is
broken** — never trust readback alone; verify with a captured frame.
`white_balance_automatic=1` is always a safe, watchable fallback.

### 2. Any open of the video node wakes the camera

Including `v4l2-ctl --get-ctrl` and V4L2 event subscriptions. The LED comes on
and the gimbal goes live. Idle monitoring must hold **no** fd — inotify on the
node works (it doesn't open the device).

This firmware has no self-sleep: the camera sleeps only via USB runtime
autosuspend (`/sys/bus/usb/devices/<port>/power/control` = `auto`), which
engages ~2 s after **all** interfaces go idle. Verified: capture stopped →
`runtime_status` returned to `suspended` within seconds. A lingering mic
capture (Slack is a known offender) keeps it awake.

### 3. Chromium resets WB on every stream open

Chromium-based apps (Chrome, Slack, Electron) write `white_balance_automatic=1`
each time they open a stream. Any WB persistence design must survive that.

### 4. `v4l2-ctl --wait-for-event` can hang forever

If the device re-enumerates, the waiting fd is dead and the call never
returns. Always wrap in `timeout(1)`.

### 5. Auto WB drowns in green rooms

Known Tiny-series weakness: in a green-painted room, auto WB overcorrects and
the image goes magenta/unusable. Manual temperature ≈4000 K is a good baseline
in such a room. Keep target temperature configurable.

## Probe log

| Date | Probe | Result |
|---|---|---|
| 2026-08-31 | Parse sysfs descriptors (no device open) | Found XU bUnitID=2 GUID `9a1e7291-6843-4683-6d92-39bc7906ee49`, 22 selector candidates; CDC ACM pair at if 4+5 |
| 2026-08-31 | MJPG 1080p capture while idle | Wakes device (`runtime_status` → `active`), re-suspends ~2 s after close |
