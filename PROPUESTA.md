# Apple Photos import for RAWmakase (macOS): proposal

Longer technical notes behind the issue for `pch/rawmakase`. The text of the issue itself is in `ISSUE.md`.

## Summary

I have a working prototype that brings a macOS Photos library into RAWmakase: originals, the
edited versions as Photos renders them, albums and folders, favourites, keywords, people,
descriptions, and the capture date as Photos shows it. It runs today next to RAWmakase 0.7.2
without any change to the app. I would like to turn it into an extension that I maintain in my
own repository, and I am asking what shape you would accept and which small core contracts it
would need.

Tested on macOS 27.0.1 (Apple Silicon) with RAWmakase 0.7.2, on a library of 12,785 photos,
1,132 videos, 72 albums and 2 folders. A dry run over the whole library finishes with no errors;
real exports were done on samples of 10 to 50 photos, not on the whole library.

## What the prototype does

Photos is read with PhotoKit (the public API), read-only. The library is never written to.

| From Photos | How it reaches RAWmakase today |
| --- | --- |
| Original file, byte for byte (HEIC, JPEG, PNG, NEF, DNG…) | Copied to `YYYY/YYYY-MM/` in a folder the user chooses; the folder is added to the catalog |
| RAW of a RAW+JPEG pair | Copied beside the JPEG |
| Edited version, as Photos renders it (full resolution) | Copied beside the original as `NAME_editada.ext` |
| Apple's adjustment data (`com.apple.photo` plist) | Kept in `_fotos/ajustes/`, unread, for a later conversion |
| Favourite | Keyword `Favorito` in an XMP sidecar |
| Keywords, title, description | XMP sidecar (`dc:subject`, `dc:title`, `dc:description`) |
| Named people | Keywords under `Personas` (`lr:hierarchicalSubject`) |
| Capture date as Photos shows it, with its time zone | `exif:DateTimeOriginal` in the sidecar |
| Location | `exif:GPSLatitude` / `exif:GPSLongitude` in the sidecar |
| Albums and folders, with each album's manual order | `create_collection` / `add_to_collection` over the control socket, photos sent in the album's order (they get positions 1, 2, 3…, so Custom Order shows Photos' order) |
| Live Photo video, videos (optional) | Copied; RAWmakase ignores them |

Checked in RAWmakase 0.7.2: photos are added, keywords and the `Personas` hierarchy are read from
the sidecars, capture times match Photos (including a photo taken at −04:00), and an album with
edited photos arrives as one collection holding originals and edited versions. A second run of
the collections step creates no duplicates.

Not checked: descriptions and locations (no sample photo had one), folders of albums, and a
full-library run.

Titles, descriptions, keywords, people and the time zone are not in PhotoKit. They are read from
the library's `Photos.sqlite`, read-only. That schema is private and changes between macOS
releases, so the reader looks column names up in the database instead of assuming them, records
in its manifest the macOS version, whether that version was verified by hand, and which columns
it used, and exports the rest when something is missing. Only macOS 27 is verified so far.

## What does not carry over

- **Photos' edits as editable settings.** Apple's adjustments run in Apple's own pipeline. The
  edited render is carried, and the adjustment data is kept, but nothing is converted to develop
  settings. Crop and rotation could be converted later; tone and colour only approximately.
- **Original and edited version as a stack**, and bursts as stacks: nothing outside the app can
  make a stack today. A small control command for it is offered below.
- Smart albums' rules, shared albums, memories, faces' regions, hidden photos (optional flag).
- Libraries other than the System Photo Library (PhotoKit reads only that one).

## Proposed shape

**Now, with `api = 3`:** an extension with a `[[command]]` ("Import from Apple Photos…") and a
`[[source]]`. Its program copies the files and reports them with `source.report`, so the app adds
them and answers with their photo ids; no "Add Folder" step for the user. Collections are then
made from those ids.

**Not built on `[[importer]]`.** I read #1116 and #966: the importer contribution was built and
then closed as not worth its complexity, so this proposal does not depend on it and does not ask
for it back. Everything here uses what `api = 3` has. What that leaves out (stacks for original +
edited version and for bursts) needs one small control command, offered below as a separate
pull request.

## What it would need from the core

1. **`NSPhotoLibraryUsageDescription` in the macOS `Info.plist`.** macOS attributes a Photos
   request to the app that started the process. Measured here: the same PhotoKit binary is
   authorised when launched on its own as an app bundle, and is denied at once, with no prompt,
   when run as the child of an app whose `Info.plist` lacks the key (tried with the bundle's
   binary and with a bare binary carrying its own embedded `Info.plist`). RAWmakase's
   `packaging/macos/Info.plist` has no such key. With it, the prompt would name RAWmakase and
   appear only when the import is first used. The alternative that needs no core change is for
   the extension to ship a small helper `.app` and start it through Launch Services; it works,
   but the prompt names the helper and the helper needs signing of its own.
2. **A way for an extension's program to make collections from photo ids.** Today that is the
   control socket, which the user must turn on. Is there, or would you accept, a request for it
   in the extension protocol?
3. **A `stack` control command**, offered as its own pull request (written, see below): it
   groups given photos into stacks through the Library's Group into Stack, several stacks as one
   undoable step. The extension uses it when `capabilities` lists it, and otherwise leaves the
   edited versions beside their originals.

## Questions

- Would you take this as an extension listed as official, or should it stay third-party?
- The PhotoKit part is Swift today (about 400 lines). Would you want it in Rust (`objc2-photos`),
  or is a Swift helper inside the package acceptable?
- Does `source.report` read XMP sidecars as Add Folder does? The docs say the files are added "as
  one import batch"; I have not tested it.
- Should a favourite be a keyword, a pick flag or a rating?
- For the edited version: is a second photo beside the original (as now) acceptable?

## The `stack` command (separate pull request)

`{"cmd":"stack","stacks":[[top, …], …]}` and `rawmakase control stack <ids…>`. Each list becomes
one collapsed stack with its first photo on top, through `StackChange::Groups`, which Auto-Stack
already uses, so the catalog's rules are untouched; all or none, one step for Undo. Refused with
`invalid_request`: a stack of fewer than two photos, a photo named twice, photos of different
folders (naming the two). About 60 lines in a new `src/app/shell/commands/stack.rs`, plus the
request, the CLI subcommand, `docs/automation.md`, and tests at each layer. No MCP tool is added.

## State of the prototype

- `extension/`: the extension itself (Rust, SDK at tag v0.7.2, `api = 3`): a `[[source]]`, the
  commands "Import from Apple Photos" and "Stop the Apple Photos Import", and settings for the
  destination folder, one album only, videos, bursts and hidden photos. Run as a developer
  extension in RAWmakase 0.7.2 on one album: the helper copied 27 files, `source.report` added
  27 photos, the album was made as a collection of 27; a second run added nothing and made no
  duplicate. Packaged as `.rmext` (0.7 MB) with the helper app included, installed with
  `rawmakase extensions install` into a clean data folder, and run from there on an album inside
  a folder: 4 photos, the set and its collection. Not run on a whole library.
- One thing found while packaging: files added with `--include` are unpacked without their
  executable bit (only the program keeps it), so the helper's binary arrives as `rw-r--r--`. The
  program sets it again before starting the helper. Would you rather the package kept the bit?

- `exportar/main.swift`: the exporter (PhotoKit + read-only `Photos.sqlite`), resumable.
- `crear_colecciones.py`: makes the collections over the control socket (protocol 1).
- Manifest `_fotos/manifiesto.json`, version 2: photos, albums with their order, the options
  used, and the origin block (macOS version, verified or not, columns used).
- Messages and file names are in Spanish for now; they would be English in the extension.
