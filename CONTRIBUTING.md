# Contributing

Thanks for helping! The most valuable contribution right now is **testing on
Tiny 3 models other than the Tiny 3 Lite**.

## Tiny 3 / Tiny 3 SE owners: please test

This suite is verified on an **OBSBOT Tiny 3 Lite** (`3564:ff04`). The Tiny 3
and Tiny 3 SE almost certainly speak the same protocol, but we haven't confirmed
it on real hardware. If you own one, please help:

1. **Find your USB product ID** (paired with the vendor, per device):
   ```bash
   for d in /sys/bus/usb/devices/*/idVendor; do
     [ "$(cat "$d")" = 3564 ] && echo "3564:$(cat "${d%idVendor}idProduct")"
   done
   # or, if you have usbutils:
   lsusb | grep -i remo
   ```
   The vendor is `3564` (Remo Tech). Note the 4-hex-digit product ID.

2. **Try the low-impact commands first** (these don't move the gimbal):
   ```bash
   t3ctl power     # truly read-only — reads sysfs, opens nothing
   t3ctl status    # reads camera state; does NOT wake a sleeping camera
   t3ctl info      # serial + UUID
   ```
   Note: `status`/`info` do send a vendor request *frame* to the camera's
   command mailbox (a documented, non-destructive GET on the Tiny 2). Only
   `t3ctl power` touches nothing on the device.

3. **Then the fun ones**, watching the camera:
   ```bash
   t3ctl sleep     # LED should go off, gimbal should park
   t3ctl wake      # LED on, gimbal lifts
   t3ctl track on ; t3ctl track off
   t3ctl pan 15 ; t3ctl recenter
   t3ctl wb temp 4000
   ```

4. **Open an issue** with: your exact model, the USB product ID, firmware
   revision (`t3ctl info`), and which commands worked / misbehaved. Frame
   captures (`ffmpeg -f v4l2 -i /dev/video…`) of anything weird help a lot.

### Adding a new model's product ID

Auto-detection already matches any `/dev/v4l/by-id` node containing
`OBSBOT_Tiny_3`, so `t3ctl` should find your camera with no change. Only the
**udev rule** needs your product ID for the group-access + cold-plug WB pin.
Add a line to [`packaging/udev/71-obsbot-tiny3.rules`](packaging/udev/71-obsbot-tiny3.rules)
mirroring the `ff04` entry with your PID, and send a PR.

## Development

- Rust, one dependency (`libc`). `cargo build` / `cargo test`.
- `cargo test` runs the CRC and frame-codec unit tests offline (no hardware).
- Please keep the firmware-quirk comments intact and add to
  [`PROTOCOL.md`](PROTOCOL.md) when you discover something — the documentation
  is as much the point of this project as the tools.

## Safety when probing the protocol

- **Never blind-sweep the vendor command surface.** On the Tiny 2, a single pass
  over all GET opcodes took the device off the USB bus. Probe in small batches,
  liveness-check between commands, and save results incrementally.
- Treat the Upgrade subsystem (beyond serial/UUID reads), BLE, and any
  firmware-flash commands as off-limits without a recovery plan.
- Verify image-affecting changes with an actual captured frame, not just control
  readback — readbacks can look correct while the image is broken.
