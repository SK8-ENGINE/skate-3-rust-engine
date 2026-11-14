#!/usr/bin/env bash
# Muted NPC runs for every on/off combination of the given env switches at several spots; prints one row per run.
# usage: npc_matrix.sh <out-dir> <seconds> "<ENV_A> <ENV_B>" "<MAP:TELEPORT> ..."   (MAP alone = its default spawn; EXE= as npcsim.sh)
OUT="$1"; SECS="$2"; read -ra KNOBS <<< "$3"; read -ra SPOTS <<< "$4"
HERE="$(cd "$(dirname "$0")" && pwd)"; mkdir -p "$OUT"
n=${#KNOBS[@]}
for spot in "${SPOTS[@]}"; do
  for ((mask=0; mask<(1<<n); mask++)); do
    names=""; envs=()
    for ((b=0; b<n; b++)); do if (( mask>>b & 1 )); then envs+=("${KNOBS[b]}=1"); names+="${KNOBS[b]}+"; fi; done
    tag="${spot//:/_}.m$mask"
    MAP="${spot%%:*}"; T=""; [[ "$spot" == *:* ]] && T="${spot#*:}"
    env "${envs[@]}" MAP="$MAP" TELEPORT="$T" bash "$HERE/npcsim.sh" "$EXE" "$OUT/$tag" "$SECS" >/dev/null
    L="$OUT/$tag.err"
    printf '%-46s %-44s bails %3s giveups %3s lost %3s back %3s land %4s tricks %4s panics %s\n' "$spot" "${names:-none}" \
      "$(grep -ac 'NPC_SKATER_BAIL #' "$L")" "$(grep -ac 'stage 4 -> 5' "$L")" "$(grep -ac 'NPC_SKATER_LOST' "$L")" \
      "$(grep -ac 'NPC_SKATER_BACKWARDS #' "$L")" "$(grep -ac 'NPC_SKATER_LAND ' "$L")" "$(grep -ac 'NPC_SKATER_TRICK ' "$L")" "$(grep -ac panicked "$L")"
  done
done
