# Custom Music

Listen to your own music while you skate. Your songs can replace the game's soundtrack or play mixed in with it.
When one of your songs starts, its name and artist show at the bottom of the screen, like a music video.

## Step 1: add your music (about 2 minutes, plus packing time)

Songs can be MP3, FLAC, M4A / AAC, OGG, WAV or AIFF. Only the songs directly in the folder you pick are added, not
the ones in folders inside it: pick each of those folders separately.

### On Windows

1. Close the game if it is open.
2. Open the game's folder, then the **mods** folder, then the **custom-music** folder (the folder this file is in).
3. Double-click **ADD-MY-MUSIC.bat**.
   - If Windows shows a blue box saying "Windows protected your PC", click **More info**, then **Run anyway**.
4. A window opens asking you to pick a folder. Click the folder that holds your songs and click **OK**.
5. Wait. A black window lists each song as it is added. A big folder can take a few minutes.
6. When it says **Done**, press any key to close the window.

### On Linux or macOS

1. Close the game if it is open.
2. Open the game's folder, then the **mods** folder, then the **custom-music** folder (the folder this file is in).
3. Open a terminal in this folder:
   - Linux: right-click an empty spot in the folder and choose **Open in Terminal** (the name differs a little
     between systems).
   - macOS: right-click the **custom-music** folder in Finder and choose **New Terminal at Folder**.
4. Type `sh add-my-music.sh` and press Enter.
5. A window opens asking you to pick a folder. Pick the folder that holds your songs.
   - If no window opens, the terminal asks you to type the folder's path instead: drag the folder into the terminal
     window and press Enter.
6. Wait. The terminal lists each song as it is added. A big folder can take a few minutes.
7. When it says **Done**, press Enter to close.

### Good to know

You can do this again for more folders. Every folder you add goes into the same playlist.

Your music is kept in the **Skate Music** folder inside your **Documents** folder (not in the mod), so sharing this
mod never shares your music. To remove a folder's songs, delete its .bank file there, then add any folder you keep
once more (that rebuilds the playlist).

## Step 2: turn the mod on

1. Start the game.
2. Open the menu (Esc on the keyboard, or Guide / F1), then **Mods**.
3. Turn on **Custom Music**.

Your music starts playing. In the mod's settings you can change:

- **Mode**: **mix** plays your songs mixed in with the game's soundtrack; **replace** plays only your songs.
- **Shuffle**: on plays your songs in a random order.
- **Volume**: 1 is the same loudness as the game's music; lower is quieter.
- **Show title and artist**: turn the song name at the bottom of the screen on or off.

Turning the mod off gives the music back to the game.

## If something goes wrong

- **No music plays**: the screen says "Custom Music: playlist not found". Do step 1 again and wait for **Done**.
- **A song is missing**: it may be in a folder within the folder you picked (see step 1), or the file could not be
  read (the window shows "skipped" next to it).
- **The song names look wrong**: the name and artist come from the song file's own information (its tags). Without
  tags, a file named `Artist - Title.mp3` works too.
- **"Could not find the game"**: this mod must stay in the game's **mods** folder.

## Sharing

A .bank file holds the songs you packed into it, so please keep banks with copyrighted music to yourself. Sharing
this mod is fine: your music is not in it.

## For advanced users

The helper runs the game's own tools, which you can also run yourself from a command window in the game's folder
(on Linux and macOS the program is `./skate3rust`):

    skate3rust.exe --pack-music-folder "<music folder>" "<bank file>.bank"
    skate3rust.exe --music-bank-info "<bank file>.bank"
    skate3rust.exe --music-playlist "mods\custom-music\playlists\my-music.json" "My music" --shuffle "<bank file>.bank" ...

A playlist is a JSON file; you can remove or reorder tracks, or add a game soundtrack track as `{"soundtrack": 12}`.
The **Playlist** setting picks another playlist file inside this mod's folder.
