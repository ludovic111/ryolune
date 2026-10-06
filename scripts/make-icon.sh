#!/bin/sh
# Render ryolune's app icon from its source, desktop/icons/ryolune.svg (written by
# scripts/gen-mark.py: the mark in white on the lsuite v2 near-black tile), into
#   desktop/assets/ryolune.icns   macOS (iconutil packs it)
#   desktop/icons/icon.png        512 px, Linux and the README
#   desktop/icons/icon.ico        Windows, 16 to 256 px (Pillow packs it)
# Needs resvg (cargo install resvg), iconutil (macOS) and python3 with Pillow.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/desktop/icons/ryolune.svg"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
set_dir="$work/ryolune.iconset"
mkdir "$set_dir"
for size in 16 32 128 256 512; do
  resvg -w $size -h $size "$src" "$set_dir/icon_${size}x${size}.png"
  double=$((size * 2))
  resvg -w $double -h $double "$src" "$set_dir/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$set_dir" -o "$root/desktop/assets/ryolune.icns"
resvg -w 512 -h 512 "$src" "$root/desktop/icons/icon.png"
resvg -w 256 -h 256 "$src" "$work/ico.png"
python3 - "$work/ico.png" "$root/desktop/icons/icon.ico" <<'PY'
import sys
from PIL import Image
Image.open(sys.argv[1]).save(sys.argv[2], sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
PY
echo "Wrote desktop/assets/ryolune.icns, desktop/icons/icon.png and desktop/icons/icon.ico"
