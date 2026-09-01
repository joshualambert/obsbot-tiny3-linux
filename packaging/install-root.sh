#!/usr/bin/env bash
# Root-scope installer for obsbot-tiny3-linux: udev rule + system config.
# Run once with sudo:  sudo ./packaging/install-root.sh
#
# The udev rule's cold-plug WB pin calls t3ctl, so t3ctl must be on root's PATH
# (i.e. installed to /usr/bin or /usr/local/bin). If you only installed t3ctl to
# ~/.local/bin, either install it system-wide too, or drop the RUN+= line from
# the rule (the t3-wb-guard daemon still handles the stream-open case).
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "run me with sudo: sudo $0" >&2
    exit 1
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "==> installing udev rule"
install -Dm644 "$REPO/packaging/udev/99-obsbot-tiny3.rules" \
    /etc/udev/rules.d/99-obsbot-tiny3.rules

echo "==> installing system config (kept if it exists)"
if [[ ! -f /etc/obsbot-tiny3/config ]]; then
    install -Dm644 "$REPO/packaging/config.example" /etc/obsbot-tiny3/config
else
    echo "    (existing /etc/obsbot-tiny3/config left untouched)"
fi

echo "==> reloading udev rules"
udevadm control --reload-rules
udevadm trigger --action=add --subsystem-match=video4linux

echo "Done. (A replug also applies the rule.)"
