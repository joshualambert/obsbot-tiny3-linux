#!/usr/bin/env bash
# Local (non-AUR) installer for obsbot-tiny3-linux.
#
# Installs the binaries, a per-user systemd unit, and a default config into the
# user's home — NO sudo required. The udev rule and system config need root and
# are handled separately by packaging/install-root.sh (run that with sudo).
#
# Usage: ./install.sh [--prefix DIR]   (default prefix: ~/.local)
set -euo pipefail

PREFIX="${HOME}/.local"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        -h|--help) echo "usage: $0 [--prefix DIR]"; exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

# Resolve to an absolute path — systemd rejects a relative ExecStart.
PREFIX="$(realpath -m "$PREFIX")"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINDIR="${PREFIX}/bin"
UNITDIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

echo "==> building release binaries"
( cd "$REPO" && cargo build --release --locked )

echo "==> installing binaries to $BINDIR"
install -Dm755 "$REPO/target/release/t3ctl" "$BINDIR/t3ctl"
install -Dm755 "$REPO/target/release/t3-wb-guard" "$BINDIR/t3-wb-guard"

# No config is seeded here: the guard defaults to 4000K, and the system-wide
# default lives at /etc/obsbot-tiny3/config (installed by install-root.sh). Only
# create ~/.config/obsbot-tiny3/config yourself to override the temperature.

echo "==> installing systemd user unit"
mkdir -p "$UNITDIR"
# Generate the unit with an absolute ExecStart matching this prefix.
cat > "$UNITDIR/t3-wb-guard.service" <<EOF
[Unit]
Description=OBSBOT Tiny 3 white-balance guard (re-pins manual WB when apps flip it to auto)
Documentation=https://github.com/joshlambert/obsbot-tiny3-linux

[Service]
Type=simple
ExecStart=${BINDIR}/t3-wb-guard
Restart=always
RestartSec=5
NoNewPrivileges=true

[Install]
WantedBy=default.target
EOF

echo "==> enabling t3-wb-guard.service"
systemctl --user daemon-reload
systemctl --user enable --now t3-wb-guard.service

echo
echo "Done. t3ctl and t3-wb-guard installed to $BINDIR"
echo "Next (root, one-time) for the udev rule + cold-plug WB pin:"
echo "    sudo $REPO/packaging/install-root.sh"
echo "Make sure $BINDIR is on your PATH."
