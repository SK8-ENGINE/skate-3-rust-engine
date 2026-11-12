# 45. Ghidra function names for Skate 3 (TU3)

**Problem.** Every retail function in a fresh Ghidra project is `FUN_xxxxxxxx`. Each decode started from unnamed callers and callees, and anyone else reading the TU3 code had to rediscover what we had already worked out.

**Change.** `tools/ghidra/skate3-tu3/`: the names of 9,644 functions decoded during this port, as a list anyone can load into their own Ghidra project (README there, written for a first-time Ghidra user):
- `skate3-tu3-symbols.txt` for Ghidra's built-in `ImportSymbolsScript.py` (nothing extra to install);
- `skate3-tu3-functions.tsv` (address, name, one-line description);
- `ApplyNames.java`, an optional script that applies the TSV with each description as a plate comment, skips addresses inside other functions and never overwrites names the user set (run from the Script Manager, it asks for the file; headless takes `<tsv> <log>`).

Names are `Subsystem_What`; where the game's Lua bindings name a function, that name is used (e.g. `FrontEnd_SetEndChallengeOption`). Only functions whose role our research established are listed; names, addresses and descriptions only, no game code.

**Verification.** Applied to the TU3 project (276 named, 4 created where no function started, 2 skipped as mid-function addresses); a decompile of `0x82DF18B0` reads `Score_UpdatePlayers` calling `Score_DispatchPlayerEvents` and `MsgBus_Post`.

**Files.** `tools/ghidra/skate3-tu3/README.md`, `skate3-tu3-symbols.txt`, `skate3-tu3-functions.tsv`, `ApplyNames.java`.

**Open.** The list grows with each decode.

Credits: Ghidra, the Xenon360 extension, skate3recomp / rexglue / Xenia.
