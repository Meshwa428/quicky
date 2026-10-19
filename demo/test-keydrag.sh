#!/usr/bin/env bash
# One held drag with Alt / Ctrl / Shift tapped mid-drag, dropped on PNG.
source "$(dirname "${BASH_SOURCE[0]}")/demo-lib.sh"
helper_start
rm -f "$D/beach.png"
omarchy-shell shell hide meshwa.oma-quicky >/dev/null 2>&1
hyprctl dispatch 'hl.dsp.focus({ workspace = "9" })' >/dev/null
pause 0.8
guard
( sleep 7; grim "$S/kd.png" ) &
KEYS="Alt_L Control_L Shift_L" dragto beach.jpg 0 7 2 5
wait
pause 3
ls "$D" | grep beach
