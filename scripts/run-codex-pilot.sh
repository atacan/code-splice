#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"

operating_system="$(uname -s)"
architecture="$(uname -m)"

case "$operating_system/$architecture" in
  Linux/x86_64)
    filesystem="$(findmnt -n -o FSTYPE --target "$repository_root")"
    test "$filesystem" = ext4
    ;;
  Darwin/arm64)
    device="$(df "$repository_root" | tail -n 1 | awk '{print $1}')"
    filesystem="$(mount | awk -v device="$device" '$1 == device { value=$4; sub(/^\(/, "", value); sub(/,$/, "", value); print value }')"
    test "$filesystem" = apfs
    ;;
  *)
    printf 'unsupported pilot row: %s/%s\n' "$operating_system" "$architecture" >&2
    exit 1
    ;;
esac

pilot_root="${CODESPLICE_PILOT_ROOT:-$repository_root/target/phase10-pilot}"
test "$(basename "$pilot_root")" = phase10-pilot
scenario_fifteen="$pilot_root/scenario-15"
cross_device="$scenario_fifteen/external"
mkdir -p "$scenario_fifteen/src" "$cross_device"

cleanup_cross_device() {
  case "$operating_system" in
    Linux)
      if mountpoint -q "$cross_device"; then
        umount "$cross_device"
      fi
      ;;
    Darwin)
      if mount | grep -Fq " on $cross_device "; then
        hdiutil detach "$cross_device" -quiet
      fi
      ;;
  esac
}
trap cleanup_cross_device EXIT

case "$operating_system" in
  Linux)
    mount -t tmpfs -o size=16m codesplice-phase10 "$cross_device"
    ;;
  Darwin)
    image="$pilot_root/cross-device.dmg"
    if test -e "$image"; then
      rm -f "$image"
    fi
    hdiutil create -quiet -size 16m -fs APFS -volname codesplice-phase10 "$image"
    hdiutil attach "$image" -quiet -nobrowse -mountpoint "$cross_device"
    ;;
esac

export CODESPLICE_PILOT_ROOT="$pilot_root"
export CODESPLICE_PILOT_CROSS_DEVICE="$cross_device"
export CODESPLICE_PILOT_BASELINE="$(git rev-parse HEAD)"
export CODESPLICE_PILOT_OS="$operating_system"
export CODESPLICE_PILOT_ARCH="$architecture"
export CODESPLICE_PILOT_FILESYSTEM="$filesystem"

cargo test -p codesplice-cli --test codex_pilot -- --ignored --exact \
  codex_pilot_should_pass_all_fifteen_scenarios --nocapture

test -s "$pilot_root/evidence.json"
printf 'pilot passed: %s/%s on %s\nevidence: %s\n' \
  "$operating_system" "$architecture" "$filesystem" "$pilot_root/evidence.json"
