# 1. ISO extraction argument order

## Problem

Selecting a Skate 3 ISO in setup failed with `Conversion failed. See ...\extract.log`
right after "Extracting your ISO". Setup deletes its temporary directory on
failure, so the log named in the message no longer exists.

## Root cause

`extract-xiso` (the pinned `build-202505152050` Windows release, v2.7.1) stops
parsing options at the first image path. Setup called it as

```
extract-xiso -x <iso> -d <dir>
```

so `-d` and `<dir>` were treated as two more images to extract. The tool
extracted the real ISO into the **current working directory** as
`<iso name>\` (not the temporary directory), then failed on the "images":

```
open error: -d No such file or directory
```

and exited with code 1, which setup reports as a conversion failure. The
extraction itself was complete: all 103 files and 6,404,940,920 bytes matched
`extract-xiso -l`.

## Change

Pass options before the image, in both call sites:

- `tools/asset_pipeline/customiser_setup.py` (`install`, used by the setup GUI)
- `tools/asset_pipeline/install.py` (`_install`)

```
extract-xiso -x -d <dir> <iso>
```

Note: `-d` does not create parent directories. Both call sites extract into a
subdirectory of an existing temporary/work directory, so this holds.

## Verification

- Reproduced the failure by running the tool with the original argument order
  (stderr above, exit code 1, files written to the working directory).
- With the new order: exit code 0, all 103 files in the target directory,
  byte total identical to the image listing, nothing written to the working
  directory.

## Notes for upstream

- Upstream PR #9 (Linux/macOS port) contains the same fix, credited there to
  PR #7 ("BSD getopt compatibility"). If #9 is merged first, this change is
  already covered.
