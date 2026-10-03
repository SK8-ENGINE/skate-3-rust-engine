#!/bin/sh
# usage: RECOMP_GENERATED=<dir> fn.sh ADDR -> prints the asm comments of function sub_ADDR (8 hex digits, upper case)
# Needs your own skate3recomp build: RECOMP_GENERATED = its generated/ folder (the code generator's output).
: "${RECOMP_GENERATED:?set RECOMP_GENERATED to your skate3recomp generated/ folder}"
cd "$RECOMP_GENERATED" || exit 1
f=$(grep -l "DEFINE_REX_FUNC(sub_$1)" skate3_recomp.*.cpp | head -1)
awk -v s="DEFINE_REX_FUNC(sub_$1)" 'index($0,s){p=1;print;next} p&&/^DEFINE_REX_FUNC/{exit} p&&(/^\t\/\/ /||/^loc_/){sub(/^\t\/\/ /,"  ");print}' "$f"
