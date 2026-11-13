---
name: ghidra
description: Read retail Skate 3 (TU3) code as Ghidra pseudocode instead of PowerPC assembly - where to get the latest Ghidra, JDK and Xenon360 (XEX/XEXP loader, VMX128) and how to check for updates, setup from scratch on your own copy, the decompile tool and headless wrapper, an optional language patch for five missing VMX128 instructions, and the known gaps. Use whenever a function of the retail game must be read or understood (research agents too), when the Ghidra project must be built or re-imported, or when Ghidra, Xenon360 or the patch must be installed, updated or re-applied.
---

# Ghidra: TU3 pseudocode

## 1. What it is for
- Pseudocode of retail TU3 functions (the `sub_XXXXXXXX` addresses of the recomp) instead of reading PowerPC by hand.
  A **reading aid** next to the recomp's generated code (skill `recomp-research`) and the traces: those stay the
  evidence. Use it to understand a function quickly, find fields and constants, and decide what to hook or port.
- **Your own copy:** the program is imported from your own install of your legally owned game. Nothing from the game,
  the Ghidra project or the decompiler output is shared or committed.
- **Reference only:** never copy pseudocode (or game code) into the repo; describe behaviour in your own words.
  Addresses, offsets and constants are facts and fine.
- **Check before porting:** names and types are guesses (`FUN_` names, `param_n`); confirm offsets, constants and loop
  exits against the generated recomp code (`skate3_recomp.*.cpp`, `DEFINE_REX_FUNC(sub_X)`), especially vector code.
- Credits: Ghidra (NSA, Apache-2.0); Xenon360 by EveryoneEverybody (Apache-2.0); skate3recomp, rexglue and Xenia for
  the addresses and the generated code this is checked against. Credit them in docs and PRs that use findings.

## 2. Where to get the latest
Last checked with `gh` on 2026-10-16 (re-check before installing; versions move):

| Piece | Source | Latest at the check | Asset |
|---|---|---|---|
| Ghidra | https://github.com/NationalSecurityAgency/ghidra/releases | `Ghidra_12.1.4_build` (2026-09-21) | `ghidra_12.1.4_PUBLIC_20260921.zip` |
| Xenon360 | https://github.com/EveryoneEverybody/Xenon360/releases | `v0.7.0` "Requires Ghidra 12.1.4" (2026-09-30) | `ghidra_12.1.4_XenonVMX128_0.7.0.zip` + `.zip.sha256` |
| JDK | https://adoptium.net (Eclipse Temurin) or https://www.oracle.com/java/technologies/downloads/ | 21 or newer for Ghidra 12.1 | x64 JDK installer or zip |

Check for newer versions:
```powershell
gh release list -R NationalSecurityAgency/ghidra -L 3
gh release list -R EveryoneEverybody/Xenon360 -L 3
gh api repos/EveryoneEverybody/Xenon360/releases/latest --jq '.tag_name, .published_at, .body, (.assets[] | .name + " " + .digest)'
```
- **Match the versions:** each Xenon360 zip is built for one exact Ghidra version (the version is in the asset name
  and the release text). Install the Ghidra version the newest Xenon360 release names, not simply the newest Ghidra.
- **JDK version:** read `application.java.min` in `<ghidra-install>\Ghidra\application.properties` (21 for 12.1).
- **SHA-256:** the release API lists a `digest` (`sha256:...`) per asset; Xenon360 also ships a `.sha256` file.
  Compare with `(Get-FileHash <zip> -Algorithm SHA256).Hash`.
- Xenon360 README (install, features, limits): https://github.com/EveryoneEverybody/Xenon360#readme; its SLEIGH sources
  are under `src/languages/`; report problems at https://github.com/EveryoneEverybody/Xenon360/issues.

Why Xenon360 and not the alternatives:
- zeroKilo/XEXLoaderWV (https://github.com/zeroKilo/XEXLoaderWV) and its fork SaveEditors/XEXLoaderWV: XEX loaders
  only, onto stock PowerPC without VMX128, so vector code does not decode. GitHub detects no licence on zeroKilo's
  repo (latest `12.1.3`, for Ghidra 12.1.3; the fork's latest is `13.0.0`).
- freeqaz/ghidra-xenon-extension: has VMX128 (including the five instructions patched below), but no licence file,
  last release `v12.1` (2026-02-11), a different language id (switching means a new import), and it likely keeps the
  same `lvrx128` overlap with ISA 3.0 `vcmpneh.`.
- Xenon360: Apache-2.0, XEX and XEXP (title update) loading, language `PowerPC:BE:64:Xenon-VMX128-32addr` with VMX128,
  PDB/XDB symbols, compiler-helper call fixups.

Learn more:
- Headless options: `<ghidra-install>\support\analyzeHeadlessREADME.html` (or `.md`). Getting started:
  `<ghidra-install>\docs\GettingStarted.md`, cheat sheet `docs\CheatSheet.html`.
- SLEIGH (for the language patch): `<ghidra-install>\docs\languages\html\sleigh.html` and `sleigh_constructors.html`.
- VMX128 encodings: Xenia's table https://github.com/xenia-project/xenia/blob/master/tools/ppc-instructions.xml
  (generated into `src/xenia/cpu/ppc/ppc_opcode_table_gen.cc`); the recomp's per-instruction `// mnemonic` comments
  confirm which instruction sits in a function.

## 3. Setup from scratch
1. **JDK:** install a JDK at least `application.java.min` (step 2 checks it); set `JAVA_HOME` or put `java.exe` on
   PATH.
2. **Ghidra:** unzip the release into `<ghidra-install>` (a folder outside the repo with a few GB free). Start once
   with `ghidraRun.bat` to confirm the JDK is found.
3. **Xenon360:** GUI: File > Install Extensions > `+` > the zip, then restart Ghidra. Headless: unzip the zip into
   `<ghidra-install>\Ghidra\Extensions\` (gives `...\Extensions\XenonVMX128\`).
4. **Tools:** install this skill per `tools/claude-skills/README.md`, so its `tools\` folder is at
   `<repo>\.claude\skills\ghidra\tools\` (`decompile.ps1` finds `<repo>` from its own path). Set
   `GHIDRA_INSTALL_DIR=<ghidra-install>` and `GHIDRA_PROJECT_DIR=<ghidra-project-dir>` (a folder outside the repo).
5. **Import TU3 with the title update applied and analyse** (`<game-install>` = the folder with `default.xex` and
   `default.xexp`):
```powershell
$t = "<repo>\.claude\skills\ghidra\tools\ghidra_headless.ps1"
$i = "<game-install>"
& $t -MaxMem 12G "<ghidra-project-dir>" "skate3-tu3" -import "$i\default.xexp" -loader XenonXexLoader `
    -loader-base-xex "$i\default.xex" -overwrite > "<ghidra-project-dir>\analysis.log"
```
   Gives program `default.xexp` = the TU3 image at the same addresses as the recomp's `sub_` names. Full analysis
   takes about 8-9 minutes and the project grows to a few GB. Loader options (headless names, from the extension jar):
   `-loader-base-xex`, `-loader-symbol-file`, `-loader-symbol-mode`. `-noanalysis` imports only;
   `-process default.xexp` re-runs analysis on the imported program. Afterwards count "Unable to resolve constructor"
   in the log (section 5).
- **Why the wrapper:** `analyzeHeadless.bat` breaks option names with spaces or parentheses (the loader's "Base XEX
  path (required for XEXP)"). `ghidra_headless.ps1 [-MaxMem 8G] <AnalyzeHeadless args>` makes the same Java call as
  `support\launch.bat` (launch.properties VM args, `-Xmx`, `ghidra.Ghidra ...AnalyzeHeadless`) without the batch layer.

## 4. Daily use
- `decompile.ps1 [-Out <file>] <addr> [addr ...]` (`82E0E580`, `0x82E0E580` or `sub_82E0E580`): pseudocode of the
  functions at the addresses into `-Out` and printed. Default `-Out` is `<repo>\.local\research\ghidra\decompile.txt`
  (keep it gitignored); a function is created if analysis missed one (the recomp's `sub_` names are function starts).
  Optional `-ProjectDir` / `-ProjectName` / `-Program` (defaults `GHIDRA_PROJECT_DIR`, `skate3-tu3`, `default.xexp`).
- About 10 s per call, mostly JVM start: **batch the addresses** in one call. Always pass your own `-Out` when several
  people or agents share a checkout.
- **One headless Ghidra at a time** (the project is locked while one runs). Check first:
  `Get-CimInstance Win32_Process -Filter "Name='java.exe'" | Where-Object { $_.CommandLine -match 'AnalyzeHeadless' }`.
- `scripts\DecompileAt.java` is the post-script `decompile.ps1` runs (`-postScript DecompileAt.java <out> <addr>...`).
- Reading the output: `FUN_xxxxxxxx` = the recomp's `sub_xxxxxxxx`; `DAT_` / `_DAT_` = globals; `in_` registers are
  values Ghidra could not trace to a parameter; `xenon_*` calls are deliberate userops or call-fixup injections
  (section 5). Then confirm every offset and constant you will use in the recomp function of the same address.
- When you delegate research to a background agent, name this skill and `decompile.ps1` in the brief and have it pass
  `-Out` with its own file.

## 5. Known quirks and the optional VMX128 language patch
- `xenon_lvlx_partial` / `xenon_lvrx_partial` (and the `stvlx` / `stvrx` ones): deliberate opaque userops = an
  unaligned 16-byte load / store at ea, not decode errors. `xenon_lvlx_partial(_DAT_x, 8)` = a misaligned load at
  x+8. The usual `lvlx | lvrx` pair is one unaligned 16-byte load.
- The GPR save / restore helpers show as `xenon_ppc_savegpr_noop` injections (the extension's call fixup).
- OPEN: the vector save / restore helpers (`__savevmx_*` / `__restvmx_*`, 164 entries at 0x82F56E60..0x82F57384, one
  every 8 bytes) have no such fixup, so a caller can lose `this` or its parameters (seen in `sub_82E35838`). Untested
  idea: add a per-program call fixup with the same no-op body as the GPR one (Edit > Options for program >
  Specification Extensions, or a headless script), create a function at each entry and set it as their Call Fixup.
- Loops can lose their exit or tail in pseudocode (seen in `82D2D550`): read the asm for the loop end.
- Without the patch below: `altv300_23(...)` = `lvrx128` (right half of an unaligned load), `altv300_24(...)` =
  `lvewx128`, and `halt_baddata()` near vector code = one of the `vpk*128` packs.

**The patch.** Xenon360 v0.7.0 has no constructors for `vpkshus128`, `vpkuhum128`, `vpkuwum128` (not decoded:
`halt_baddata`, "Unable to resolve constructor", flow truncated) and decodes `lvrx128` / `lvewx128` on v32..v63
silently as POWER9 `vcmpneh.` / `vcmpnew`, whose results are dropped: 389 sites in 54 TU3 functions (mostly vector and
memory-copy helpers). The extension already removes other POWER9 overlaps the same way (stock `vabsdub`, `vcmpneb.`,
`vcmpnezb.` for `lvlx128` / `stvlx128`). The patch is SLEIGH text written for this setup, not game code. Files:
`<ghidra-install>\Ghidra\Extensions\XenonVMX128\data\languages\`; back that folder up first.
1. In `xenon_altivec.sinc` comment out three ISA 3.0 constructors (Xenon has none), each with a note:
   ```
   # Local patch: stock vabsduh removed, its encoding overlaps lvrx128 on Xenon.
   # :vabsduh vrD,vrA,vrB        is OP=4 & vrD & vrA & vrB & XOP_0_10=1091 {
   #     vrD = altv300_16(vrA,vrB);
   # }

   # Local patch: stock vcmpneh. removed, its encoding overlaps lvrx128 v32..v63 on Xenon.
   # :vcmpneh. vrD,vrA,vrB        is OP=4 & vrD & vrA & vrB & Rc2=1 & XOP_0_9=71 {
   #     vrD = altv300_23(vrA,vrB);
   # }

   # Local patch: stock vcmpnew removed, its encoding overlaps lvewx128 on Xenon.
   # :vcmpnew vrD,vrA,vrB        is OP=4 & vrD & vrA & vrB & Rc2=0 & XOP_0_9=135 {
   #     vrD = altv300_24(vrA,vrB);
   # }
   ```
   (`vcmpnew.` with `Rc2=1` stays.)
2. New file `xenon_vmx128_phase7.sinc`, and `@include "xenon_vmx128_phase7.sinc"` after the phase6 include in
   `xenon_64_isa_altivec_be.slaspec`:
   ```
   # Local patch: VMX128 instructions missing from Xenon360 v0.7.0 (packs and two loads).

   :vpkshus128 xvD,xvA,xvB
       is OP=5 & xvD & xvA & xvB & XENON_BITS_6_9=9 & XENON_BIT_4=0
   {
       xvD = vectorPackSignedHalfWordUnsignedSaturate(xvA,xvB);
   }

   :vpkuhum128 xvD,xvA,xvB
       is OP=5 & xvD & xvA & xvB & XENON_BITS_6_9=12 & XENON_BIT_4=0
   {
       xvD = vectorPackUnsignedHalfWordUnsignedModulo(xvA,xvB);
   }

   :vpkuwum128 xvD,xvA,xvB
       is OP=5 & xvD & xvA & xvB & XENON_BITS_6_9=14 & XENON_BIT_4=0
   {
       xvD = vectorPackUnsignedWordUnsignedModulo(xvA,xvB);
   }

   :lvrx128 xvR,RA_OR_ZERO,RB
       is OP=4 & xvR & RA_OR_ZERO & RB & BITS_6_10=17 & BITS_4_5=0
   {
       ea:$(REGISTER_SIZE) = RA_OR_ZERO + RB;
       eb:1 = ea[0,4];
       aligned:$(REGISTER_SIZE) = ea & 0xfffffffffffffff0;
       block:16 = *[ram]:16 aligned;
       xvR = xenon_lvrx_partial(block,eb);
   }

   :lvewx128 xvR,RA_OR_ZERO,RB
       is OP=4 & xvR & RA_OR_ZERO & RB & BITS_6_10=2 & BITS_4_5=0
   {
       xvR = loadVectorElementWordIndexed(RA_OR_ZERO,RB);
   }
   ```
   Patterns follow the extension's `lvlx128` (phase4) and `vpkswss128` / `vpkuwus128` (phase4 / phase6); the pack
   userops are the stock ones; `xenon_lvrx_partial` is already defined in phase4, so do not define it again.
3. Recompile: with `JAVA_HOME` set, run `<ghidra-install>\support\sleigh.bat` on
   `...\languages\xenon_64_isa_altivec_be.slaspec` (writes the `.sla` beside it). Expect only the stock warnings; a
   "conflict" error means a pattern is wrong.
4. Re-import with `-overwrite` and re-analyse (section 3 step 5): an existing project keeps the old undecoded spots.
5. Verify after every re-import: `decompile.ps1 82a4b888 82edc5b0 827f0b78 82b09288`. Expect no `halt_baddata`, no
   `altv300_*`, `xenon_lvrx_partial` beside `xenon_lvlx_partial`; the log should report 0 "Unable to resolve
   constructor" (25 without the patch).

**A new Xenon360 release** replaces these files and drops the patch. Before installing it, unzip it into a temporary
folder and search its `XenonVMX128\data\languages\` for `vpkshus128`, `vpkuhum128`, `vpkuwum128`, `lvrx128`,
`lvewx128` (constructors start with `:`), and for the colliding `:vcmpneh.` / `:vcmpnew ` / `:vabsduh` in
`xenon_altivec.sinc`:
```powershell
Select-String -Path "<tmp>\XenonVMX128\data\languages\*.sinc" -Pattern '^\s*:(vpkshus128|vpkuhum128|vpkuwum128|lvrx128|lvewx128|vcmpneh\.|vcmpnew|vabsduh)\s'
```
(`^\s*:` skips commented-out lines.) If all five are there and the three collisions are gone, skip the patch; otherwise re-apply steps 1-5. Reporting the
gap at the Xenon360 issues page beats carrying the patch forever.

## 6. Updating Ghidra itself
- Only update when a Xenon360 release for the new Ghidra version exists (section 2); Ghidra will not load an
  extension built for another version.
- Unzip the new Ghidra into a new folder beside the old one, install the matching Xenon360 (and the patch if still
  needed), point `GHIDRA_INSTALL_DIR` at it, re-import into a new project folder, run the verify step, then delete the
  old install and project. Keep the old ones until the new one is verified.
- A changed language (new Xenon360, patch applied or removed) needs a re-import with `-overwrite`; reopening the old
  project does not re-decode existing code.
