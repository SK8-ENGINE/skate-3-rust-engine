# Simon Says 2.0

Enable this version on every player, using the updated game build. The host
starts/stops from **Gamemodes > Simon Says**. Everyone races to land the called
trick; the first confirmed clean ride-away observed by the host wins the round.
Others lose a life. Solo plays as survival against the clock. Category filters,
round timing, escalation, sudden death, hints and the call card remain settings.
Eliminated and late-joining players spectate a surviving player. Stop, match end,
world changes and unloading release the camera and player suspension.

Each player acknowledges a new round before its results can count. The host
reads replicated engine landing/bail counters and settled scoring metadata;
there is no stick-gesture win detector or client-submitted win claim. Body spins
are separate from board shuvit rotation. A bail during ride-away cancels credit.
All peers need this version and the new engine observation fields.

Caveman and Acid Drop guide entries are excluded from random calls because the
current extracted scoring catalog has no distinct settled labels for them.
The duplicate FS Fastplant card is de-duplicated. Ground tricks must finish and
produce a confirmed scoring result; merely entering a manual/plant does not win.

Start/Stop now use the game menu instead of the old ready/reset hotkeys. The
obsolete stick-detector minimum-airtime setting has been removed.
