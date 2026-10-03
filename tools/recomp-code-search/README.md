# Recomp code search

**For use with the Skate 3 recomp's research hooks:**
[Hailey-Ross/skate3recomp, branch `research-hooks`](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks)
(described in [`docs/hails-additions/13-recomp-research-hooks.md`](../../docs/hails-additions/13-recomp-research-hooks.md)).

> **Reference only.** These scripts search the recompiled sources and the game's memory image that
> *you* generate from your own legally owned copy of the game. You must build the recomp yourself and
> set up the paths below for your machine; nothing from the game is included here. Use what you find
> as a reference for re-implementing behaviour, never copy game code into the engine.

Finding the code behind a behaviour is the first step before hooking it (see the research-hooks
branch for the hooks themselves).

| Script | What it does |
|---|---|
| `fn.sh ADDR` | Prints the disassembly comments of the recompiled function `sub_ADDR`. |
| `grepfn.sh 'ERE'` | Prints `sub_XXXXXXXX: <asm line>` for every disassembly line matching the pattern, with its function. |
| `callctx.sh ADDR [before] [after]` | Every call site of `sub_ADDR` (first 20), with the surrounding disassembly and the calling function. |
| `ppcxref.py` | Cross-references in a big-endian PowerPC memory image: code that builds an address (`lis` + `addi` / load / store), strings and their users, callers of a function, the function containing an address, and data words pointing at an address (vtables, tables). |

## Inputs

- `RECOMP_GENERATED`: your skate3recomp build's `generated/` folder (the code generator's output,
  `skate3_recomp.*.cpp`), for the shell scripts.
- For `ppcxref.py`: `--image` (or `PPC_IMAGE`), a dump of the executable's loaded memory image from
  your own copy, base `0x82000000` by default (`--base`); `--funcs` (or `PPC_FUNCS`), a list of function
  starts built from your generated sources (the command is in the script's help); optional
  `--code LO-HI` to scan only the code section.

## Usage

```
export RECOMP_GENERATED=/path/to/skate3recomp/generated
sh tools/recomp-code-search/fn.sh 82XXXXXX
sh tools/recomp-code-search/grepfn.sh 'lfs .*,-?[0-9]+\(r31\)'
sh tools/recomp-code-search/callctx.sh 82XXXXXX 6 14

py -3.13 tools/recomp-code-search/ppcxref.py --image image.bin --funcs funcs.txt str some_string
py -3.13 tools/recomp-code-search/ppcxref.py --image image.bin --funcs funcs.txt calls 82XXXXXX
```

## Example output

`ppcxref.py calls` (made-up addresses):

```
82123450 in sub_82123400
82345670 in sub_82345600
```

## Requirements

Git Bash or any POSIX shell with `grep` / `awk` for the shell scripts; Python 3.13 (standard library)
for `ppcxref.py`.
