#!/usr/bin/env bash
# Synthesizes Quicky's UI sounds with ffmpeg.
#
# Each sound is a crisp, damped click: broad energy peaking at 4–8 kHz over an
# even broadband floor, with nothing ringing. Narrow or resonant filters are
# what make a click sound like a fingernail on glass, so every filter here is
# a gentle first/second-order filter. Three layers:
#   body  a pitch-dropping sine thump: a little weight at 60–250 Hz
#   mid   a band-limited noise transient: the floor, 350 Hz–3 kHz
#   air   a 3–12 kHz noise burst, ~5 ms: the click itself
# Both noise layers swell in over ~2 ms rather than starting at full level —
# that soft attack is most of the difference between smooth and harsh.
# Rerun after tweaking: sounds/make-sounds.sh

set -euo pipefail
cd "$(dirname "$0")"

# click <out> <dur> <body from Hz> <body to Hz> <body gain> <mid gain> <mid cutoff Hz> <air gain> <master dB> [second-hit delay ms]
click() {
  local out=$1 dur=$2 f0=$3 f1=$4 bg=$5 mg=$6 mcut=$7 ag=$8 master=$9 echo_ms=${10:-}
  # Exponential pitch drop f0→f1 over ~8 ms; phase is the integral of frequency.
  local k=260
  local body="$bg*(1-exp(-t*4000))*exp(-t*150)*sin(2*PI*($f1*t + ($f0-$f1)*(1-exp(-t*$k))/$k))"
  local mid="$mg*(random(0)*2-1)*(1-exp(-t*1100))*exp(-t*520)"
  local air="$ag*(random(1)*2-1)*(1-exp(-t*1100))*exp(-t*650)"
  local graph="[1]lowpass=f=$mcut,highpass=f=350[m];[2]highpass=f=3000,lowpass=f=12000[a];[0][m][a]amix=inputs=3:normalize=0,volume=${master}dB"
  # Optional per-sound shaping, e.g. EQ="equalizer=f=707:t=o:w=1:g=11".
  [[ -n ${EQ:-} ]] && graph+=",$EQ"
  [[ -n $echo_ms ]] && graph+=",aecho=1:0.7:$echo_ms:0.75"
  graph+=",afade=t=out:st=$(awk "BEGIN{print $dur - 0.01}"):d=0.01[out]"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "aevalsrc='$body':s=48000:d=$dur" \
    -f lavfi -i "aevalsrc='$mid':s=48000:d=$dur" \
    -f lavfi -i "aevalsrc='$air':s=48000:d=$dur" \
    -filter_complex "$graph" -map "[out]" -ac 1 -c:a pcm_s16le "$out"
}

# Hover: a crisp 4–8 kHz click over an even broadband floor.
# A one-octave lift centered on 707 Hz fills the 500 Hz–1 kHz band to match
# the rest of the floor.
EQ="equalizer=f=707:t=o:w=1:g=11" click tick.wav 0.045 230 110 0.04 0.62 3000 1.0 8

# Wheel switch: same family, darker — more floor and body, less top.
click shift.wav 0.06 170 85 0.09 0.75 2400 0.6 12

# Run: a firmer double click.
click run.wav 0.1 210 100 0.06 0.66 2800 0.95 12 42
