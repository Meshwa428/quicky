#!/usr/bin/env bash
# The scripted Quicky demo. Records the screen (with desktop audio) to
# ~/Videos/quicky-demo.mp4. Every file reaches the wheel by being dragged:
# press on it in Files, keep holding, press the shortcut, (Alt for Tools),
# and let go only on the option.
source "$(dirname "${BASH_SOURCE[0]}")/demo-lib.sh"
OUT="$HOME/Videos/quicky-demo.mp4"

helper_start

# Files on workspace 9, showing the demo folder with only the originals.
find "$D" -maxdepth 1 -type f ! \( -name beach.jpg -o -name blossom.jpg -o -name notes.pdf -o -name samurai.mp4 -o -name song.mp3 -o -name street.jpg \) -delete
omarchy-shell shell hide meshwa.oma-quicky >/dev/null 2>&1
hyprctl dispatch 'hl.dsp.focus({ workspace = "9" })' >/dev/null
hyprctl dispatch 'hl.dsp.focus({window="class:org.gnome.Nautilus"})' >/dev/null
pause 0.4
wtype -M ctrl -k l -m ctrl; pause 0.4; wtype "$D"; wtype -k Return; pause 1.5
guard
blank
pause 1

gpu-screen-recorder -w eDP-1 -k auto -f 60 -fm cfr -fallback-cpu-encoding yes -a default_output -ac aac -o "$OUT" 2>"$S/gsr.log" &
REC=$!
pause 2.5

# 1. Drag a photo onto WEBP.
dragto street.jpg 1 7 0 6 5; pause 2.5

# 2. Mid-drag, Alt / Ctrl / Shift switch between Tools, Actions, and
#    Convert; drop on PNG.
KEYS="Alt_L Control_L Shift_L" dragto beach.jpg 0 7 2 5; pause 2.2

# 3. Edit Photo: Alt mid-drag, drop on Edit.
KEY=Alt_L dragto street.jpg 2 7 1; win quicky.edit; pause 1.4
wdrag 197 377 207 377                       # exposure
wdrag 197 449 214 449                       # contrast
wdrag 197 485 178 485                       # highlights
wdrag 197 521 222 521                       # shadows
wglide 60 560
for i in 1 2 3 4 5 6; do m scroll -1; pause 0.07; done; pause 0.5
wdrag 197 410 210 410                       # temperature
wdrag 197 518 222 518                       # vibrance
wdrag 197 611 222 611                       # clarity
pause 0.8
wclick 331 688; gone quicky.edit; pause 1.2

# 4. Annotate.
KEY=Alt_L dragto street.jpg 3 7 2; win quicky.annotate; pause 1.2
wdrag 190 420 296 362                       # arrow at the cat
wclick 211 85; wdrag 272 330 350 392        # box round it
wclick 165 85; wclick 40 285; pause 0.3
wtype -d 70 "Cat on the roof"; pause 0.5; tap Escape
wclick 341 85; wclick 330 300; pause 0.3; wclick 120 380
pause 1
wclick 320 688; gone quicky.annotate; pause 1.2

# 5. Add BG.
KEY=Alt_L dragto blossom.jpg 4 7 3; win quicky.addbg; pause 1.4
wclick 66 369; pause 0.6; wclick 138 369; pause 0.6; wclick 174 369; pause 0.5
wdrag 151 544 190 544                       # corners
wdrag 151 584 195 584                       # shadow
wclick 229 623; pause 1.2                   # 1:1
wclick 316 688; gone quicky.addbg; pause 1.2

# 6. Collage: three selected photos dragged together.
WITH="blossom.jpg street.jpg" KEY=Alt_L dragto beach.jpg 2 3 1; win quicky.collage; pause 1.6
wclick 193 325; pause 1; wclick 356 325; pause 1.2
wclick 339 688; gone quicky.collage; pause 1.2

# 7. Trim a video.
KEY=Alt_L dragto samurai.mp4 3 9 1 2; win quicky.trim; pause 2.2
wdrag 24 457 110 457
wdrag 396 457 320 457
tap l; pause 3; tap space; pause 0.5
wclick 372 609; gone quicky.trim; pause 1.2

# 8. Audio visualizer.
KEY=Alt_L dragto song.mp3 3 7 2; win quicky.visualize; pause 1.2
wclick 36 392; pause 3.5
wclick 320 476; pause 0.6; tap Down; pause 0.2; tap Return; pause 3
wclick 36 392; pause 0.8
wclick 317 528; gone quicky.visualize; pause 1.2

# 9. Organize a PDF.
KEY=Alt_L dragto notes.pdf 3 4 2; win quicky.organize; pause 1.8
wdrag 342 200 70 200; pause 0.8
wclick 388 272; pause 0.8
wclick 28 560; pause 0.6
wclick 324 609; gone quicky.organize; pause 1

# Everything that was made.
blank; pause 3
echo "saved $OUT"
