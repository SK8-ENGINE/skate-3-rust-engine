# Third-party notices

Bevy sources retain their MIT and Apache licenses under `vendor/`.

`tools/vendor/utt` contains the MIT-licensed UTT RX2/model parsers by duckyinnit.
The full license is alongside the source. No UTT game payloads or executables
are bundled.

The map exporters and archive readers come from the MIT-licensed Skate 3
Custom Engine Layer. See `tools/vendor/university/LICENSE-PROJECT.md`.
The supplied preview.18 exporter has an explicit lossless v14 storage mode for
this runtime. The character scripts are the project's existing clone converters;
the ABIN/RX2 toolkit provenance is in `tools/vendor/skate3_anim/PROVENANCE.md`.

The AttribSys converter uses the publicly documented VaultLib layout as a
reference: https://github.com/NFSTools/VaultLib. Its lookup8 hash and record
reader preserve data from the user's BIN/VLT files; no settings payload is
bundled.

Release setup is paused pending direct asset conversion. Blender is not
downloaded by setup. The retained authoring scripts can use a separately
supplied Blender executable. XboxDev extract-xiso retains its own license
files. Game assets are never downloaded from this project.

The setup executable bundles Python, NumPy, Pillow, Tcl/Tk and PyInstaller's
bootloader. Their license information is retained by the packager alongside
its bundled libraries. No game content is included in the release archive.
