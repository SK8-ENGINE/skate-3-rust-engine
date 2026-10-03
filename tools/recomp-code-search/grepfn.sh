#!/bin/sh
# usage: RECOMP_GENERATED=<dir> grepfn.sh 'ERE' -> prints "sub_XXXX: asm line" for every asm line matching (grep -E syntax)
# Needs your own skate3recomp build: RECOMP_GENERATED = its generated/ folder (the code generator's output).
: "${RECOMP_GENERATED:?set RECOMP_GENERATED to your skate3recomp generated/ folder}"
cd "$RECOMP_GENERATED" || exit 1
grep -n -E "DEFINE_REX_FUNC\(sub_|^	// .*($1)" skate3_recomp.*.cpp | awk '
/DEFINE_REX_FUNC\(sub_/ { match($0,/sub_[0-9A-F]+/); f=substr($0,RSTART,RLENGTH); next }
{ sub(/^[^\t]*\t\/\/ /,""); print f": "$0 }'
