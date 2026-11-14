# Skate 3 (TU3) function names for Ghidra

Names for 28,172 functions of the Xbox 360 Skate 3 executable with Title Update 3, worked out while porting Skate 3 to this engine. Load them into your own Ghidra project and the decompiler shows `Score_UpdatePlayers` instead of `FUN_82df18b0`.

This folder holds names and short descriptions only. It contains no game code and no game files.

**Confirmed names and guesses.** 11,423 names are confirmed: what the function does was worked out while porting it. The other 16,749 are guesses: their names start with `guess_` (for example `guess_ParkSetup_ReleaseLists`) (most descriptions also start with "Guess:"). A guess is a best reading of the code that nobody has checked against the game yet, so treat it as a hint. When a guess is confirmed it loses the prefix.

## What is in here

| File | What it is |
|---|---|
| `skate3-tu3-symbols.txt` | One function per line (`name address f`), for Ghidra's built-in `ImportSymbolsScript.py`. |
| `skate3-tu3-functions.tsv` | The same names with a one-line description each: `address`, `name`, `comment`, separated by tabs. |
| `ApplyNames.java` | An optional Ghidra script that applies the TSV and also puts each description above its function as a comment. |

## What you need

1. [Ghidra](https://github.com/NationalSecurityAgency/ghidra/releases) (made with 12.1.4).
2. The [Xenon360](https://github.com/EveryoneEverybody/Xenon360/releases) Xbox 360 extension for Ghidra (made with v0.7.0).
3. Your own copy of the game's `default.xex` plus the Title Update 3 patch (`default.xexp`), loaded as one program at its normal base address `0x82000000`. The addresses only match TU3.

## Load the names (the simple way)

1. Open your Skate 3 program in Ghidra and let the analysis finish.
2. Open **Window > Script Manager**.
3. Type `ImportSymbolsScript` in the filter box and double-click **ImportSymbolsScript.py**.
4. Pick `skate3-tu3-symbols.txt` when it asks for a file.
5. Save the program (**File > Save**).

The names show up in the symbol tree, the listing and the decompiler right away.

## Load the names with their descriptions (optional)

1. In **Window > Script Manager**, click the **Manage Script Directories** button (the list icon) and add this folder.
2. Find **ApplyNames.java** (category `Skate3`) and double-click it.
3. Pick `skate3-tu3-functions.tsv`.
4. Save the program.

It writes a log next to the TSV (`skate3-tu3-functions.tsv.log`). Addresses that fall inside another function, or functions you already named yourself, are skipped and listed in that log, so it never overwrites your own work.

## Naming

`Subsystem_What`, for example `HudTimer_Update`, `Score_DispatchPlayerEvents`, `MsgBus_Post`. Where the game's own Lua bindings give a function a name (for example `FrontEnd_SetEndChallengeOption`), that name is used. The list grows as more of the game is decoded.

## Credits

Ghidra (NSA), the Xenon360 loader, and the skate3recomp / rexglue / Xenia projects, whose work made reading the TU3 code possible.
