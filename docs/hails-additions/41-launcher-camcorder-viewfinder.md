# 41. Launcher: the camcorder's LCD panel

Status: in #52 with the rest of the fork launcher (`tools/steam-launcher`, `world/replay`).

## Problem

The Skate launcher looked like a generic dark console menu. The goal (USER-ASKS 484 to 490) is a launcher that feels
like skate culture through the flip-out LCD panel of a mid-2000s HD camcorder, seen first-person: the window is
only the screen and its bezel (no camera body), it swings open from the hinge at the start and shut at the close, and
the panel looks like it has been through years of sessions. Earlier steps (an overlay, then the whole camera on a
ledge) were replaced by this.

## Change

- **The window is the LCD panel:** a borderless per-pixel-alpha (layered) window, 62 % of the work area's height,
  drawn in a 1240 x 760 unit space: the molded panel frame with a raised lip, the dark gasket round the glass, the
  screen, the backlight sensor, and the hinge barrel on the right edge where the camera body would be in your hand.
  Drag it by its bezel. Every frame is drawn into one bitmap and handed over with `UpdateLayeredWindow`
  (`Native.Push`).
- **The bezel's keys are the controls** (clickable; the pad and keyboard work as before):

  | Key | Does |
  | --- | --- |
  | HOME, bezel STOP | Back (B) |
  | Zoom rocker T / W | Up / down |
  | SET, bezel PLAY | Select (A) |
  | Bezel REW / FF | Left / right (step the list) |
  | START/STOP | Play the selected version on the main menu, select elsewhere |
  | POWER | Close (the panel swings shut first) |

- **First-person open and close:** the panel swings out from edge-on at the hinge (perspective strips, the free
  edge receding while it is turned away), settles facing you with a little wobble past flat, then the backlight comes
  on, sometimes with a flicker. Every visible close (POWER, Exit, the auto-close after a session, a close from Steam
  or the taskbar, which arrive as `None` / `TaskManagerClosing`) plays it backwards, then the window closes. A hidden
  close (self-update restart) skips it. Each open and close rolls its own swing time, wobble, screen-on delay and
  flicker (`Vary`).
- **Standby around a game (no power-off):** when the game's window shows, the panel folds shut with the screen still
  lit (REC and the tape counter stay up, no backlight fade), then hides and hands the game the foreground. When the
  game exits it swings back open already lit, mid recording (no logo, no backlight coming on); the recording stops
  (STBY, counter held) once it settles. Only the launcher's own start plays the power-on. Both swings use a shot of
  the lit screen in place of the dark one (`Fold`, `Resume`, `LitFlip`); a close during either falls back to the
  power-off.
- **The screen is a mid-2000s HD camcorder's UI** (drawn at the screen's own pixels, smooth type): the status row
  (STBY / REC with its glowing dot while a job runs, an HDV 1080i badge, the tape counter that only runs while
  recording, a glossy battery), a blue HOME / title tab, glass panels, the list with an amber glossy selection bar,
  the selected row's info (READY / NOT AVAILABLE pills, what it does, what it records), the job screen (glossy
  progress bar for cargo's step count, counts, the first error or the newest log lines), text pages, the power-on
  logo with the start check's step, A / B / START/STOP badges for the keys and the date and time stamp. With Play
  selected the info panel ends in a small play prompt in the same badge style (a pulsing record dot, the START/STOP
  badge, "to play"), not a big button. Behind the
  menus the camera points at a dimmed skate spot at dusk. The skateboard rides a lane along the bottom of the screen
  and pops an ollie now and then (its speed, place and rhythm rolled per start).
- **Wear, rolled fresh at every start:** paint-marker scrawl in skate-session words ("SESH", "LAST TRY", "NO COMPLY",
  "SECURITY!!", "SKATE & ANNOY", ...), doodles (crown, star, arrow, X, smiley), scratch clusters, paint rubbed off
  along the edges, now and then gaff tape across a bottom corner. All of it stays on the **wear map**: the bezel minus
  keep-out zones over every UI element (glass, keys and their labels, print, grille, sensor). Tags and doodles also stay clear
  of the sticker (a tag on the bottom strip moves to the sticker's roomier side) and of each other (a gap around every
  one drawn before), and shrink until their whole outline, pen and smoothing included, fits; short words go to the
  small spot. A doodle whose rolled place is taken moves to the nearest free place in either spot, or is left out.
- **The sticker:** one sticker slapped on the bezel's bottom strip years ago, picked once per user at the first start
  (design, place, angle, wear) and kept in `camcorder-sticker.txt` in the state folder. Worn: faded, scratched, grimy,
  chipped, one edge peeled up and folded back with old glue under it, and creased along the gasket edge and the
  bezel's lip it was pressed over. The set: an original "filmer" character on clear vinyl in the style of hand-drawn
  riso character stickers (die cut: the art's silhouette grown by a 6-unit vinyl margin, holes filled, its outer edge
  traced and smoothed once per start, `FilmerCut`), and parodies of skate brand stickers (Blickey, Spitball Wheels, Supremely Mid, Thrashed).
- **Game stickers replace the parody set:** PNGs in the state folder's `stickers\` folder are used instead when there
  are any (same place and angle, creases too). Finding the retail stickers and exporting them there is the
  living-world TODO `.claude/todo/living-world-stickers.md`; nothing from the game is committed.

## Cost

Measured with `SkateLauncher.exe --bench <out.txt> [WxH]` at 1052 x 645: about 3 ms per frame during the swing and 7 ms
settled, plus about 1.2 ms to hand the frame to the window. That came from:
- The panel's face is drawn once per size on the first (invisible) frame and cut into 48 strips; the swing only draws
  the strips (each with a pixel of overlap so they meet without seams), nearest-neighbor while moving. Drawing one
  big image per strip, a texture brush, source rectangles, image attributes or bilinear sampling each cost 20 to
  110 ms a frame in GDI+.
- The LCD canvas is copied 1:1 at whole pixels and cleared every frame (the scene's edge pixels are not fully opaque,
  so an uncleared canvas built up the last frame along its top and left edge).
- The dusk scene is cached per size.

The window redraws every timer tick while it animates and every second tick (about 30 fps) otherwise; nothing while a
game runs (the launcher hides).

## Verification

- `SkateLauncher.exe --wear-test <out.txt> <out.png>`: one start's wear roll, checked: the same frame drawn twice is
  identical, every pixel the wear changes is on the wear map, no tag, doodle or tape crosses a keep-out zone, and no
  tag or doodle sits under the sticker or on another one (each test start rolls its own sticker too).
  Run 30 times as 30 separate starts: 30 of 30 PASS.
- `SkateLauncher.exe --standby-test <out.txt> <out.png>`: the game-start fold and the game-exit reopen, 7 frames each:
  the screen's middle is lit on every frame with the panel up, and the fold ends with nothing drawn. PASS.
- `SkateLauncher.exe --snapshot <out.png> <screen>[@<0..1>][#<seed>] [WxH]`: any screen, `@` a point in the power-on,
  `#` the sticker and wear from that seed (snapshots never write the sticker file).
- Live window runs (screen captures of the window only): the swing open, the power-on screen, the menu, the close
  (gone after about 1 s), the sticker kept between runs.
- Built with the Windows csc (C# 5) like `build.bat`, no warnings.

## Open / next

- Mouse clicks on the LCD rows and the bezel keys are mapped (`LoHit`, `Key`) but not yet clicked through by hand.
- More sticker designs; the game's own stickers once found (living-world TODO).
- The hinge is on the right; a left-hinge option if wanted.
