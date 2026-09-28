#!/usr/bin/env bash
# Optional PipeWire call microphone. Does not restart the audio server.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
command -v pipewire >/dev/null
mkdir -p "$CONFIG/obsbot-tiny3" "$CONFIG/systemd/user"
for path in "$CONFIG/obsbot-tiny3/obsbot-audio.conf" "$CONFIG/systemd/user/t3-audio.service"; do
    if [[ -f "$path" ]]; then cp -p "$path" "$path.bak.$(date +%s)"; fi
done
install -m644 "$REPO/packaging/pipewire/obsbot-audio.conf" "$CONFIG/obsbot-tiny3/obsbot-audio.conf"
# Use the actual XDG path, including spaces, for systemd's argument parsing.
sed "s|%h/.config/obsbot-tiny3/obsbot-audio.conf|\"$CONFIG/obsbot-tiny3/obsbot-audio.conf\"|" \
    "$REPO/packaging/systemd/t3-audio.service" > "$CONFIG/systemd/user/t3-audio.service"
systemctl --user daemon-reload
systemctl --user enable t3-audio.service
systemctl --user restart t3-audio.service
printf '%s\n' 'Select OBSBOT Calls (Noise + Echo Reduction) as your call microphone.'
