# Helpers for the scripted Quicky demo. Source it.
S=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)   # this folder: mouse.py, fifos, logs
Q=~/.config/omarchy/plugins/meshwa.oma-quicky/bin/quicky
D="$HOME/Videos/Quicky Demo"
CX=683 CY=410          # where the wheel opens
R=100                 # radius to a segment's label

# Send a command and wait for its own ack (stale acks from an interrupted
# run are skipped).
m() {
  local id=$RANDOM$RANDOM got
  echo "$id $*" >"$S/m.fifo"
  while read -r got <"$S/m.fifo.ack"; do [[ $got == "$id" ]] && break; done
}
pause() { sleep "${1:-1}"; }

# The virtual mouse lives only as long as the script: whatever way the
# script ends (done, guard, error, interrupt), the helper and any
# recording stop with it.
helper_start() {
  rm -f "$S/m.fifo" "$S/m.fifo.ack"; mkfifo "$S/m.fifo" "$S/m.fifo.ack"
  python3 "$S/mouse.py" "$S/m.fifo" >"$S/mouse.log" 2>&1 &
  HPID=$!
  trap cleanup EXIT INT TERM
  sleep 1.2
}
cleanup() {
  trap - EXIT INT TERM
  [[ -n ${HPID:-} ]] && kill "$HPID" 2>/dev/null
  [[ -n ${REC:-} ]] && kill -INT "$REC" 2>/dev/null && wait "$REC" 2>/dev/null
  exit
}

# Where FILE's icon sits in the Files grid (6 columns), from the folder as it
# is now. Files sorts "name.ext" before "name-suffix.ext", case-insensitively.
# Stops the script unless Files shows the demo folder: never click or drag
# someone's real files.
guard() {
  local t
  t=$(hyprctl -j clients | jq -r '[.[] | select(.class=="org.gnome.Nautilus" and .workspace.id==9)][0].title // ""')
  [[ $t == "Quicky Demo" ]] || { echo "Files shows '$t', not Quicky Demo; stopping" >&2; exit 1; }
}

icon() {
  python3 - "$D" "$1" <<'PY'
import os, sys
names = sorted(os.listdir(sys.argv[1]), key=lambda n: n.lower().replace(".", "\x01"))
i = names.index(sys.argv[2])
print(382 + 174 * (i % 6), 160 + 138 * (i // 6))
PY
}

# (No hyprctl focus here: focusing warps the pointer to the window centre.)
pick() { sleep 0.4; guard; m click $(icon "$1"); }      # select one file
add() { guard; wtype -M ctrl -s 1; m click $(icon "$1"); wtype -m ctrl; }  # ctrl-click
blank() { m click 820 600 300; }                         # clear the selection

# The wheel opens centred on the pointer, shifted inward to stay on screen
# (Quicky.qml: a 290 px ring plus a 46 px caption on a 1366×768 screen).
center_at() {
  CX=$(( $(clamp $(( $1 - 145 )) 0 $((1366 - 290))) + 145 ))
  CY=$(( $(clamp $(( $2 - 145 )) 0 $((768 - 290 - 46))) + 145 ))
}
clamp() { local v=$1; ((v < $2)) && v=$2; ((v > $3)) && v=$3; echo $v; }

# Copy FILE(s) (click, Ctrl+click the rest, Ctrl+C) and press the shortcut:
# the wheel opens on the copied files, right where the pointer is.
copyopen() {
  guard
  local x y f
  pick "$1"
  for f in "${@:2}"; do add "$f"; done
  pause 0.3; wtype -M ctrl -k c -m ctrl; pause 0.4
  read -r x y < <(hyprctl cursorpos | tr -d ',')
  center_at "$x" "$y"
  # What the shortcut runs outside Files: open on the clipboard.
  omarchy-shell shell toggle meshwa.oma-quicky '{}' >/dev/null; pause 0.9
}

# Point at segment I of N (clockwise from the top), optionally click it.
seg_xy() { awk -v i="$1" -v n="$2" -v cx=$CX -v cy=$CY -v r=$R 'BEGIN{a=i*2*3.14159265/n; printf "%d %d", cx+r*sin(a), cy-r*cos(a)}'; }
hover() { m glide $(seg_xy "$1" "$2") "${3:-0}"; }
choose() { hover "$1" "$2"; pause 0.35; m click $(seg_xy "$1" "$2") 120; }

# Keys go through the virtual mouse device (see mouse.py), never wtype,
# which would cancel a drag in progress.
tap() { m key "$1"; }

# Drag FILE out of Files, summon the wheel mid-drag (what Super+Alt+C does),
# wander over a few segments, and drop on segment I of N.
dragto() {
  local file=$1 i=$2 n=$3 x y f
  guard
  # WITH="a b": select those first, so dragging FILE carries them all.
  if [[ -n ${WITH:-} ]]; then
    pick "$file"
    for f in $WITH; do add "$f"; done
    pause 0.3
  fi
  read -r x y < <(icon "$file")
  m glide "$x" "$y"; pause 0.25
  m down; pause 0.15
  m glide $((x + 30)) $((y + 40)) 300         # tear the file off
  pause 0.3
  center_at $((x + 30)) $((y + 40))
  "$Q" open; pause 0.9                        # the shortcut, mid-drag
  # KEY (Alt_L for Tools), or several KEYS in turn, switch wheels while
  # still holding the file.
  for k in ${KEY:-} ${KEYS:-}; do pause 0.5; tap "$k"; pause 0.9; done
  for k in "${@:4}"; do hover "$k" "$n"; pause 0.25; done
  hover "$i" "$n"; pause 0.5
  m up
}

# Window geometry of CLASS into WX WY WW WH (waits for it to appear).
win() {
  local g
  for _ in $(seq 1 60); do
    g=$(hyprctl -j clients | jq -r --arg c "$1" '.[] | select(.class==$c) | "\(.at[0]) \(.at[1]) \(.size[0]) \(.size[1])"' | head -1)
    [[ -n $g ]] && break
    sleep 0.1
  done
  read -r WX WY WW WH <<<"$g"
  [[ -n $WX ]] || { echo "$1 didn't open; stopping" >&2; exit 1; }
}
# Click / drag at window-relative points.
wclick() { m click $((WX + $1)) $((WY + $2)) "${3:-0}"; }
wdrag() { m drag $((WX + $1)) $((WY + $2)) $((WX + $3)) $((WY + $4)) "${5:-0}"; }
wglide() { m glide $((WX + $1)) $((WY + $2)) "${3:-0}"; }
# Wait for a window to close (the tool finished).
gone() { for _ in $(seq 1 600); do hyprctl -j clients | jq -e --arg c "$1" 'any(.[]; .class==$c)' >/dev/null || return 0; sleep 0.1; done; }
shot() { grim "$S/$1.png"; }
