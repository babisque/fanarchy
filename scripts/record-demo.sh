#!/usr/bin/env bash
# Record a screen region (or take an existing video) and turn it into a
# README-sized GIF.
#
# Usage:
#   scripts/record-demo.sh [options] <out.gif>
#
# Options:
#   --from FILE.mp4    convert an existing recording instead of recording
#   --crop WxH+X+Y     crop the video first (same geometry as `magick -crop`)
#   --start SECONDS    skip the first SECONDS of the video
#   --end SECONDS      stop at SECONDS (measured from the start of the video)
#   --width PX         GIF width (default 720)
#
# Without --from it records with wf-recorder: select the region with the
# mouse, stop with Ctrl+C.
#
# Only records and converts. It never talks to fanarchyd, never runs as root
# and never touches /sys.
set -euo pipefail

FPS=12
MAX_BYTES=$((5 * 1024 * 1024))

usage() {
  sed -n '5,13p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: '$1' is not installed (sudo pacman -S $1)" >&2
    exit 1
  fi
}

from="" crop="" start="" end="" width=720 out=""
while [[ $# -gt 0 ]]; do
  case $1 in
    --from | --crop | --start | --end | --width)
      [[ $# -ge 2 ]] || usage
      case $1 in
        --from) from=$2 ;;
        --crop) crop=$2 ;;
        --start) start=$2 ;;
        --end) end=$2 ;;
        --width) width=$2 ;;
      esac
      shift 2
      ;;
    -h | --help) usage ;;
    -*) usage ;;
    *)
      [[ -z $out ]] || usage
      out=$1
      shift
      ;;
  esac
done

[[ $out == *.gif ]] || usage
[[ -z $crop || $crop =~ ^[0-9]+x[0-9]+\+[0-9]+\+[0-9]+$ ]] || {
  echo "error: --crop must look like 560x1050+1360+30" >&2
  exit 2
}
for n in "$start" "$end"; do
  [[ -z $n || $n =~ ^[0-9]+(\.[0-9]+)?$ ]] || {
    echo "error: --start/--end take seconds, like 0.5" >&2
    exit 2
  }
done
[[ $width =~ ^[0-9]+$ ]] || usage

need ffmpeg

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [[ -n $from ]]; then
  if [[ ! -s $from ]]; then
    echo "error: $from does not exist or is empty" >&2
    exit 1
  fi
  input=$from
else
  need wf-recorder
  need slurp
  echo "Select the region with the mouse; recording starts right away. Press Ctrl+C to stop."
  region=$(slurp)
  # Ctrl+C reaches wf-recorder too, which finishes the file and exits. The
  # script itself only notes the signal and goes on to convert.
  trap ':' INT
  wf-recorder -g "$region" -f "$tmp/raw.mp4" || true
  trap - INT
  if [[ ! -s $tmp/raw.mp4 ]]; then
    echo "error: nothing was recorded" >&2
    exit 1
  fi
  input=$tmp/raw.mp4
fi

# Trim with -ss/-to on the input, so both passes see the same frames.
in_args=()
[[ -n $start ]] && in_args+=(-ss "$start")
[[ -n $end ]] && in_args+=(-to "$end")

filters=""
if [[ -n $crop ]]; then
  IFS='x+' read -r cw ch cx cy <<<"$crop"
  filters="crop=${cw}:${ch}:${cx}:${cy},"
fi
filters+="fps=${FPS},scale=${width}:-1:flags=lanczos"

# 64 colours and no dithering: the panel's grainy background changes every
# frame with dithering on, which more than doubles the file size.
echo "Converting to GIF (${FPS} fps, ${width} px wide)..."
ffmpeg -v error -y "${in_args[@]}" -i "$input" \
  -vf "${filters},palettegen=stats_mode=diff:max_colors=64" "$tmp/palette.png"
ffmpeg -v error -y "${in_args[@]}" -i "$input" -i "$tmp/palette.png" \
  -lavfi "${filters}[x];[x][1:v]paletteuse=dither=none:diff_mode=rectangle" "$out"

du -h "$out"
size=$(stat -c %s "$out")
if ((size > MAX_BYTES)); then
  echo "warning: $out is over 5 MB; trim it (--start/--end) or lower --width" >&2
fi
