#!/usr/bin/env bash
# Shootout driver.
#
# Each tool is given the input its design expects, and every result is scored the
# same way: render the SVG back with rsvg-convert and compare pixels to the
# source. Node counts come from parsing the SVG, so they are tool-agnostic too.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
CASES="$ROOT/cases"
SCORE="$ROOT/target/release/score"
VTRACER="${VTRACER:-vtracer}"   # override if not on PATH

# ---------------------------------------------------------------------------
# Table 1: shape geometry, mono cases.
#
# Coverage (alpha) only, so potrace's black fill is not penalised for not being
# the source colour. Ours reads the anti-aliased PNG; potrace gets the ideal
# thresholded bilevel mask, which is the best input it can accept.
# ---------------------------------------------------------------------------
printf '== Table 1: shape geometry (mono cases) ==\n'
printf '%-16s %6s %26s %26s\n' '' '' '--------- ours ---------' '-------- potrace -------'
printf '%-16s %6s %9s %6s %8s %9s %6s %8s\n' \
  'case' 'size' 'accuracy' 'nodes' 'area' 'accuracy' 'nodes' 'area'

while IFS=$'\t' read -r name w h kind; do
  [ "$kind" = "mono" ] || continue
  src="$CASES/$name.png"

  potrace -s -o "$CASES/$name.potrace.svg" "$CASES/$name.pbm" 2>/dev/null

  line=$(printf '%-16s %6s' "$name" "${w}x${h}")
  for tool in ours potrace; do
    svg="$CASES/$name.$tool.svg"
    png="$CASES/$name.$tool.render.png"
    if [ ! -f "$svg" ]; then
      line+=$(printf ' %9s %6s %8s' '-' '-' '-')
      continue
    fi
    rsvg-convert -w "$w" -h "$h" -o "$png" "$svg" 2>/dev/null
    read -r acc worst area <<<"$($SCORE alpha "$src" "$png")"
    nodes=$($SCORE nodes "$svg")
    line+=$(printf ' %9.5f %6s %+8.0f' "$acc" "$nodes" "$area")
  done
  printf '%s\n' "$line"
done < "$CASES/manifest.tsv"

# ---------------------------------------------------------------------------
# Table 2: colour reproduction, every case, on an opaque white background.
#
# This is the fair common ground: VTracer is built for opaque input, and "logo on
# white" is the commonest real input. Full premultiplied RGBA comparison.
# potrace is excluded because it is a 1-bit tracer and cannot represent colour.
# ---------------------------------------------------------------------------
printf '\n== Table 2: colour reproduction (opaque white background) ==\n'
printf '%-16s %6s %26s %26s\n' '' '' '--------- ours ---------' '------- vtracer --------'
printf '%-16s %6s %9s %6s %8s %9s %6s %8s\n' \
  'case' 'size' 'accuracy' 'nodes' 'ms' 'accuracy' 'nodes' 'ms'

while IFS=$'\t' read -r name w h kind; do
  src="$CASES/$name.white.png"

  # Ours, on the same opaque input the competitor gets.
  start=$(date +%s%N)
  "$ROOT/target/release/ours_white" "$src" "$CASES/$name.ourswhite.svg" >/dev/null
  ours_ms=$(( ($(date +%s%N) - start) / 1000000 ))

  start=$(date +%s%N)
  "$VTRACER" -i "$src" -o "$CASES/$name.vtracer.svg" >/dev/null 2>&1
  vt_ms=$(( ($(date +%s%N) - start) / 1000000 ))

  line=$(printf '%-16s %6s' "$name" "${w}x${h}")
  for pair in "ourswhite:$ours_ms" "vtracer:$vt_ms"; do
    tool="${pair%%:*}"; ms="${pair##*:}"
    svg="$CASES/$name.$tool.svg"
    png="$CASES/$name.$tool.render.png"
    if [ ! -s "$svg" ]; then
      line+=$(printf ' %9s %6s %8s' 'fail' '-' '-')
      continue
    fi
    rsvg-convert -b white -w "$w" -h "$h" -o "$png" "$svg" 2>/dev/null
    if [ ! -s "$png" ]; then
      line+=$(printf ' %9s %6s %8s' 'render!' '-' '-')
      continue
    fi
    read -r acc worst area <<<"$($SCORE rgba "$src" "$png")"
    nodes=$($SCORE nodes "$svg")
    line+=$(printf ' %9.5f %6s %8s' "$acc" "$nodes" "$ms")
  done
  printf '%s\n' "$line"
done < "$CASES/manifest.tsv"
