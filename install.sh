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

# Two shapes of source tree land here: a published release tarball, which ships
# the binaries prebuilt next to this script, and a git clone, which does not.
if [[ -x "$REPO/t3ctl" && -x "$REPO/t3-wb-guard" ]]; then
    echo "==> using the prebuilt binaries shipped in this release archive"
    T3CTL="$REPO/t3ctl"
    WB_GUARD="$REPO/t3-wb-guard"
else
    echo "==> building release binaries"
    ( cd "$REPO" && cargo build --release --locked )
    T3CTL="$REPO/target/release/t3ctl"
    WB_GUARD="$REPO/target/release/t3-wb-guard"
fi

echo "==> installing binaries to $BINDIR"
install -Dm755 "$T3CTL" "$BINDIR/t3ctl"
install -Dm755 "$WB_GUARD" "$BINDIR/t3-wb-guard"
install -Dm755 "$REPO/bin/t3-preview" "$BINDIR/t3-preview"

# The Omarchy bar widget is intentionally NOT installed here — this installer is
# a cross-platform CLI/daemon install (works fine on Ubuntu, Debian, etc.). The
# widget is a separate, opt-in add-on; we only suggest it below if Omarchy is
# detected. See the end of this script.

# No config is seeded here: the guard defaults to 4000K, and the system-wide
# default lives at /etc/obsbot-tiny3/config (installed by install-root.sh). Only
# create ~/.config/obsbot-tiny3/config yourself to override the temperature.

echo "==> installing systemd user unit"
mkdir -p "$UNITDIR"
# Generate the unit with an absolute ExecStart matching this prefix.
cat > "$UNITDIR/t3-wb-guard.service" <<EOF
[Unit]
Description=OBSBOT Tiny 3 white-balance guard (re-pins manual WB when apps flip it to auto)
Documentation=https://github.com/joshualambert/obsbot-tiny3-linux

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

# Opt-in Omarchy/Hyprland extras — suggested, never auto-applied.
if command -v omarchy >/dev/null 2>&1 || [[ -d "$HOME/.config/omarchy" ]]; then
    cat <<'EOF'

Omarchy detected. Optional add-ons (not installed automatically):
  • Bar widget — a separate plugin repo / marketplace listing:
        omarchy plugin install io.github.joshualambert.obsbot-tiny3   # once listed
        https://github.com/joshualambert/omarchy-obsbot-tiny3         # manual install
  • Keybindings, a Camera menu, and the preview window rule are example
    snippets in this repo's packaging/hypr/ and packaging/omarchy-menu.jsonc.
EOF
elif command -v hyprctl >/dev/null 2>&1 || [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
    cat <<'EOF'

Hyprland detected. Example keybindings and the t3-preview float rule are in
this repo's packaging/hypr/ (vanilla .conf form included).
EOF
fi
