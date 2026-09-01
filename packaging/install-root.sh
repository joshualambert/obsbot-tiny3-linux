#!/usr/bin/env bash
# Root-scope installer for obsbot-tiny3-linux: udev rule + system config.
# Run once with sudo:  sudo ./packaging/install-root.sh
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "run me with sudo: sudo $0" >&2
    exit 1
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RULE=/etc/udev/rules.d/71-obsbot-tiny3.rules

echo "==> installing udev rule -> $RULE"
install -Dm644 "$REPO/packaging/udev/71-obsbot-tiny3.rules" "$RULE"

# The cold-plug WB pin RUN+= calls t3ctl, which must be at a path root can exec.
# Resolve the real location (AUR: /usr/bin; local install: ~SUDO_USER/.local/bin
# or /usr/local/bin) and rewrite the rule's RUN path to match. If none is found,
# strip the RUN line so udev doesn't log failures — the daemon still covers the
# stream-open case.
user_home=""
[[ -n "${SUDO_USER:-}" ]] && user_home="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
t3ctl_path=""
for cand in /usr/bin/t3ctl /usr/local/bin/t3ctl ${user_home:+"$user_home/.local/bin/t3ctl"}; do
    if [[ -x "$cand" ]]; then t3ctl_path="$cand"; break; fi
done

if [[ -n "$t3ctl_path" && "$t3ctl_path" != "/usr/bin/t3ctl" ]]; then
    echo "==> pointing the cold-plug WB pin at $t3ctl_path"
    sed -i "s#/usr/bin/t3ctl #${t3ctl_path} #" "$RULE"
elif [[ -z "$t3ctl_path" ]]; then
    echo "==> t3ctl not found on a root-executable path; removing the cold-plug WB-pin line"
    sed -i '/RUN+=.*t3ctl/d' "$RULE"
fi

echo "==> installing system config (kept if it exists) -> /etc/obsbot-tiny3/config"
if [[ ! -f /etc/obsbot-tiny3/config ]]; then
    install -Dm644 "$REPO/packaging/config.example" /etc/obsbot-tiny3/config
else
    echo "    (existing /etc/obsbot-tiny3/config left untouched)"
fi

echo "==> reloading udev rules"
udevadm control --reload-rules
# NB: this trigger re-applies the rule to an already-plugged camera, which fires
# the WB pin now (a control write, not a stream — it does not start streaming).
udevadm trigger --action=add --subsystem-match=video4linux

echo "Done. (A replug also applies the rule.)"
