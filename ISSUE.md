<!--
Texto del issue para pch/rawmakase, adaptado a su formulario "Feature request".
Título: Import a macOS Photos library: extension prototype, and two questions before I go further
-->

### Before submitting

- [x] I searched existing issues for similar requests. I found none about Apple Photos. I read #966 and #1116, so this does not depend on the `importer` contribution and does not ask for it back.

### What problem would this solve?

My family's photos live in the macOS Photos app: about 12,800 photos in 72 albums, 1,650 of them edited there, with favourites, keywords and named people. To use RAWmakase for them I have to export from Photos by hand, and that loses the albums, the people, the keywords, any date or place corrected in Photos, and the link between an original and its edited version. Lightroom Classic users have `Import Lightroom Catalog`; someone coming from Photos has nothing.

### What would you like RAWmakase to do?

Nothing in the core for the import itself. I wrote it as an extension, using only what `api = 3` offers, and I would maintain it in my own repository: https://github.com/omeganter/rawmakase-apple-photos

I am opening this issue so nobody duplicates the work, and to ask two things:

1. **Would you accept `NSPhotoLibraryUsageDescription` in `packaging/macos/Info.plist`?** macOS attributes a Photos request to the app that started the process. Measured on macOS 27: the same PhotoKit binary is authorised when started on its own as an app bundle, and is denied at once, with no prompt, when run as a child of an app whose `Info.plist` lacks the key. So the extension ships a small helper app and starts it through Launch Services. That works, but the permission prompt names the helper instead of RAWmakase, and the helper needs signing of its own. With the key, the extension's program could read Photos directly.
2. **Is there, or would you accept, a way for an extension's program to make collections from photo ids?** Today the extension uses the control socket for that, so albums are only made when the user has external control turned on.

Neither blocks the extension; both would make it simpler.

### Example workflow and expected result

Starting with: a Photos library, and an open RAWmakase catalog.

I would: install the extension, choose a destination folder in its settings, turn its photo source on for the catalog, and run **Import from Apple Photos**.

The expected result, which is what the prototype does today:

- every original is copied byte for byte into `YYYY/YYYY-MM/` (HEIC, JPEG, PNG, NEF, DNG…), and the edited version, as Photos renders it, beside it as `NAME_editada.ext`;
- the files are reported through `source.report`, so the app adds them with no Add Folder step;
- an XMP sidecar per photo carries the keywords, the description, named people as keywords under `Personas`, a `Favorito` keyword, the location, and the capture time as Photos shows it, with its time zone;
- each album becomes a collection and each folder of albums a collection set, with the album's manual order as the collection's Custom Order;
- running it again copies and adds nothing twice.

The Photos library is only read, never written.

### Alternatives and workarounds

- **Export from Photos by hand**, then Add Folder: loses everything listed above.
- **A standalone exporter plus a script** driving `rawmakase control`: this is how the prototype started, and it still needs a manual Add Folder. The `[[source]]` contribution removed that step.
- **Converting Photos' edits into develop settings**: not attempted. Apple's adjustments run in Apple's own pipeline. The edited render is carried and the adjustment data is kept unread, for a later conversion of crop and rotation.

### Relevant version, operating system, and camera details

RAWmakase version: 0.7.2 (extension SDK at tag `v0.7.2`)
Operating system and version: macOS 27.0.1, Apple Silicon
Other: a System Photo Library with HEIC, JPEG, PNG, NEF and DNG originals

### Additional context

**What was actually run.** Packaged as `.rmext`, installed with `rawmakase extensions install` into a clean data folder, and run on single albums of 4 to 27 photos: files added, the album made as a collection inside its set, keywords, people, descriptions, locations and corrected capture times read back from the catalog (including photos set to −05:00 and −07:00). A dry run over the whole library finishes with no errors. A full-library import (12,785 photos) is the next test; I will report its result here. Titles, bursts and hidden photos are implemented but not tested.

**What it reads outside PhotoKit.** Keywords, descriptions, people and the time zone are not in PhotoKit, so they come from the library's `Photos.sqlite`, opened read-only. That schema is private and changes between macOS releases: the reader looks column names up in the database instead of assuming them, and writes into its manifest the macOS version, whether that version was checked by hand, and which columns it used. Only macOS 27 is checked.

**Known gaps.** Original and edited version are two photos side by side, not a stack. Only the System Photo Library. Videos are copied on request and not shown. The helper is ad-hoc signed, so another Mac may refuse it.

**A found detail, in case it is unintended.** Files added to a package with `cargo rawmakase package --include` are unpacked without their executable bit; only the program keeps it. The extension sets the helper's bit again before starting it.

**A follow-up, separate from this issue.** I have a small change that adds a `stack` control command (Group into Stack for given photo ids, several stacks as one undoable step, through `StackChange::Groups`), so an import can stack an original with its edited version. It was written with Claude, and your review rule asks for a reviewer from another provider, which I will have this week. I plan to open it as a pull request on Wednesday 25 November, once it has had that review. If you would rather not have such a command at all, say so and I will not send it.

**How this was made.** The extension and this text were written with Claude (Fable 5.2), directed and tested by me on my own library.
