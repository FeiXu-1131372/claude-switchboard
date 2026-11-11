#!/bin/zsh
# Turns the raw captures into docs/screenshots: window corners, a soft
# shadow, the theme sheet and the hero. Needs ImageMagick 7.
R=${0:A:h}; RAW=${RAW:-${TMPDIR:-/tmp}/switchboard-readme-shots}; OUT=${1:-$R/../../docs/screenshots}; TMP=$RAW/tmp; mkdir -p $OUT $TMP
RADIUS=28   # the window's 14px corner radius at 2x
round() { # round <in> <out>
  local w h; read w h <<< "$(magick identify -format '%w %h' $1)"
  magick $1 \( -size ${w}x${h} xc:black -fill white -draw "roundrectangle 0,0 $((w-1)),$((h-1)) $RADIUS,$RADIUS" \) -alpha off -compose CopyOpacity -composite $2
}
shadow() { # shadow <in> <out>
  magick $1 \( +clone -background black -shadow 32x28+0+18 \) +swap -background none -layers merge +repage $2
}
win() { round $RAW/$1.png $TMP/$1.png && shadow $TMP/$1.png $OUT/$2.png && echo "  $2"; }

win sessions macos-sessions-list
win recap macos-sessions-recap
win cost macos-cost
win trends-day macos-trends-day
win breakdown macos-breakdown
win providers macos-providers
win settings macos-settings-appearance
win rotation macos-settings-rotation
win dark-breakdown macos-dark
win popover macos-popover
win popover-details macos-popover-details
win switch-confirm macos-switch-confirm
win warmup macos-warmup

# The accounts list is the sidebar of the expanded window.
magick $RAW/accounts-tall.png -crop 558x1440+0+0 +repage $TMP/accounts-crop.png
round $TMP/accounts-crop.png $TMP/accounts.png && shadow $TMP/accounts.png $OUT/macos-accounts.png && echo "  macos-accounts"

# Five colour themes and a dark one, two across.
for t in theme-terracotta theme-graphite theme-tide theme-moss theme-plum dark-trends; do round $RAW/$t.png $TMP/$t.png; done
magick montage $TMP/theme-terracotta.png $TMP/theme-graphite.png $TMP/theme-tide.png $TMP/theme-moss.png $TMP/theme-plum.png $TMP/dark-trends.png \
  -tile 2x3 -geometry 900x600+16+16 -background none $TMP/themes.png 2>/dev/null
# Flat colour, so 255 colours is enough and a third of the size.
magick $TMP/themes.png -strip +dither -colors 255 -define png:compression-level=9 $OUT/macos-themes.png; echo "  macos-themes"

# Hero: the expanded window with the popover in front of its corner.
round $RAW/trends.png $TMP/trends.png; shadow $TMP/trends.png $TMP/hero-window.png
shadow $TMP/popover.png $TMP/hero-popover.png
read ww wh <<< "$(magick identify -format '%w %h' $TMP/hero-window.png)"
read pw ph <<< "$(magick identify -format '%w %h' $TMP/hero-popover.png)"
magick -size $((ww+150))x$((wh+90)) xc:none $TMP/hero-window.png -geometry +0+0 -composite \
  $TMP/hero-popover.png -geometry +$((ww+150-pw))+$((wh+90-ph)) -composite $OUT/macos-hero.png; echo "  macos-hero"
