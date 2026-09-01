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

Read-only probe results (GET_LEN / GET_INFO / GET_CUR via `UVCIOC_CTRL_QUERY`,
2026-08-31, fw 0510): **selectors 1–22 all exist, all report length 60 bytes,
all report caps 0x03 (GET+SET)**; selectors ≥23 return ENOENT.

This XU is the **same one the OBSBOT Tiny 2 uses** (bUnitID 2, GUID
`9A1E7291-6843-4683-6D92-39BC7906EE49`, 19 controls). The GUID is not exposed
by cached sysfs on this unit but the vendor protocol below is confirmed
byte-compatible, so the Tiny 3 Lite speaks the Tiny 2 wire protocol. Two
selectors carry everything:

| Selector | Role | Confirmed on Tiny 3 Lite |
|---|---|---|
| `0x02` | Framed **V3** command channel + reply mailbox | ✅ yes |
| `0x06` | 60-byte status block; also raw-TLV write target (AI/HDR/exposure/FOV) | ✅ yes |

## The framed V3 protocol (selector 0x02) — CONFIRMED on Tiny 3 Lite

60-byte frame, zero-padded, little-endian throughout:

```
off 0    : 0xAA                     magic
off 1    : FLAGS   0x25 = SET (with nested payload) · 0x01 = header-only GET
off 2-3  : seq     u16   (reply echoes it back — match on this)
off 4-5  : len     u16 = 0x000C     (bytes 0..11 are covered by the header token)
off 6-7  : token   u16   CRC-16/USB over bytes[0:6]+00 00+bytes[8:12]
off 8    : sender  = 0x0A            (host)
off 9    : receiver                  (subsystem id: 0x02 camera, 0x03 gimbal, 0x04 AI, 0x0D upgrade)
off 10-11: cmd     u16   wire command id
--- nested payload segment, present only when there is a payload ---
off 12-13: len2    u16   payload length
off 14-15: token2  u16   CRC-16/USB over bytes[12:14]+00 00+payload
off 16.. : payload
```

**CRC-16/USB**: poly `0xA001` (reflected 0x8005), init `0xFFFF`, refin=refout=true,
xorout `0xFFFF`. Validated against nine Tiny4Linux known-good frames AND live
against this device.

**Flags byte is the key to readback.** SETs use `0x25`; **GETs must use `0x01`**
or the device returns zeros. This was verified live: `UG_GET_SN` framed with
`0x01` returned the real 14-char serial; the reply came back with flags `0x29`,
sender/receiver swapped.

**Reply mailbox rules** (all confirmed live): `SET_CUR` the request frame to
selector `0x02`, then `GET_CUR` selector `0x02` and parse. The mailbox retains
the previous reply, so **validate that reply seq AND cmd match the request**;
poll ~6–12× at 50 ms because reply latency exceeds a fixed sleep. GETs are
answered; **SET commands (sleep/wake/recenter) are fire-and-forget and return
no mailbox reply** — verify their effect by the status block or by behaviour,
never by a reply.

### Confirmed commands (live on this Tiny 3 Lite unit)

| Feature | flags | cmd | receiver | payload | Result |
|---|---|---|---|---|---|
| Get serial number | `0x01` | `0x18C8` | `0x0D` | — | ✅ `RMOWUHI3111PLN` (14 ASCII) |
| Get UUID | `0x01` | `0x1808` | `0x0D` | — | ✅ 24 bytes |
| Sleep | `0x25` | `0xA0C2` | `0x02` | `01 00 00 00` | ⏳ sent, LED verify pending |
| Wake | `0x25` | `0xA0C2` | `0x02` | `00 00 00 00` | ⏳ sent, LED verify pending |
| Gimbal recenter | `0x25` | `0x00C3` | `0x03` | `00 00 00 00 00 00` | ✅ centered (frame-verified) |

Byte order note: `cmd` bytes on the wire are little-endian, e.g. sleep = `C2 A0`.

### Raw-TLV commands (selector 0x06) — CONFIRMED

Distinct from framed V3: write a raw `[tag][len][value…]` zero-padded to 60
bytes directly to selector `0x06` (no magic, no CRC). Effect shows in the
status block.

| Tag | Control | Value | Result |
|---|---|---|---|
| `0x16` | AI tracking | `[enable][framing]`: enable `0x02` on / `0x00` off; framing 0=normal 1=upper-body 2=close-up 3=headless 4=lower-body | ✅ `16 02 02 00` enabled tracking; status byte `0x18` → `02`, frame showed re-framing |
| `0x01` | HDR / WDR | `0`/`1` | not yet tested |
| `0x03` | face-priority AE | `0` global / `1` face (needs auto-exposure on) | not yet tested |
| `0x04` | field of view | `0` wide 86° · `1` med 78° · `2` narrow 65° | not yet tested |

### Status block (selector 0x06 GET_CUR) decode

Live blob observed: `2e0100020000000100 01 78 0000 01 01 …`. Decoded offsets
(from Tiny4Linux status.rs, confirmed reacting on this unit):

| Offset | Field | Observed |
|---|---|---|
| `0x02` | sleep (0=awake, 1=sleep) | `00` awake |
| `0x06` | HDR (bool) | `00` |
| `0x18` | AI mode enable | `00` idle → `02` when tracking on |
| `0x1c` | AI framing | `00` |
| `0x21` | tracking speed (0=std, 2=sport) | `00` |

**Verification catch-22:** the sleep byte can only be read by opening the video
node, which itself may wake the camera. So the status block is **not** a
reliable sleep indicator — the physical LED / gimbal park is the ground truth
(see Probe log).

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
| 2026-08-31 | XU GET_LEN/GET_INFO/GET_CUR sweep, selectors 1–32 | 1–22 exist, uniform 60-byte length, all GET+SET; ≥23 ENOENT; non-zero payloads on 3,4,6,7,9,10,14 |
| 2026-08-31 | Framed V3 `UG_GET_SN` (flags 0x01) | ✅ reply flags 0x29, serial `RMOWUHI3111PLN` — confirms Tiny 2 V3 protocol + CRC-16/USB work on Tiny 3 Lite |
| 2026-08-31 | AI tracking on `16 02 02 00` → selector 6 | ✅ status byte 0x18 → 02; captured frame showed gimbal re-framing on subject |
| 2026-08-31 | AI tracking off `16 02 00 00` | ✅ status byte 0x18 → 00 |
| 2026-08-31 | Gimbal recenter cmd 0xC300 receiver 0x03 | ✅ frame-verified return to centered view |
| 2026-08-31 | Sleep/wake cmd 0xA0C2 receiver 0x02 | frames accepted (no reply, expected); LED-level verification pending |
