#!/bin/sh
# usage: RECOMP_GENERATED=<dir> callctx.sh ADDR [before=6] [after=14] -> every call site of sub_X (first 20)
#   with the surrounding asm and the calling function
B=${2:-6}; A=${3:-14}
# Needs your own skate3recomp build: RECOMP_GENERATED = its generated/ folder (the code generator's output).
: "${RECOMP_GENERATED:?set RECOMP_GENERATED to your skate3recomp generated/ folder}"
cd "$RECOMP_GENERATED" || exit 1
x=$(echo "$1" | tr 'A-F' 'a-f')
grep -n -E "DEFINE_REX_FUNC\(sub_|^	// bl 0x$x\$" skate3_recomp.*.cpp | awk -F: '/DEFINE_REX_FUNC/{f=$1; fn=$0; next} {print f":"$2}' | head -20 | while IFS=: read file line; do
  fnl=$(awk -v L=$line 'NR<=L && /DEFINE_REX_FUNC/{l=$0} NR==L{print l; exit}' $file | grep -o 'sub_[0-9A-F]*')
  echo "=== in $fnl"
  awk -v L=$line -v B=$B -v A=$A 'NR>=L-B*4 && NR<=L+A*4 && (/^\t\/\/ /||/^loc_/){sub(/^\t\/\/ /,"  ");print}' $file
done
