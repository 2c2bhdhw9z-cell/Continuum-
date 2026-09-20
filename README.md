# Continuum

Continuum is one app for your iPhone that plays games from lots of old consoles.

It ships as a single file called `Continuum.ipa`. You build that file in the cloud, download it
to your phone, and install it yourself, outside the App Store. That file is the whole product.
There is no website version and no App Store listing.

Inside the app are five "cores". A core is the piece of software that emulates one console
family. Five cores cover nine systems today.

## What it plays right now

| Console | Files it takes | Core doing the work |
| --- | --- | --- |
| NES | `.nes` | fceumm |
| SNES | `.sfc`, `.smc` | snes9x |
| Game Boy, Game Boy Color, Game Boy Advance | `.gb`, `.gbc`, `.gba` | mgba |
| Genesis / Mega Drive, Master System, Game Gear | `.md`, `.gen`, `.sms`, `.gg` | genesis_plus_gx |
| PlayStation 1 | `.cue`, `.chd`, `.pbp`, `.iso` | pcsx_rearmed |

`.bin` files can also be imported, but you never tap a `.bin` to play it. A PlayStation `.cue`
file is a small text file that lists the `.bin` track files sitting next to it, so those `.bin`
files have to be in the app for the `.cue` to load.

## Where it actually stands

Read this part before you expect the app to work.

**Confirmed:**

- The app builds successfully in the cloud.
- All five cores are confirmed to be inside the `.ipa`. The build checks each one by name and
  goes red if any is missing.
- Importing game files on a real phone works.
- On a real phone the drawing path ran at 60 fps with 0 dropped frames, and roughly 6.84 GB of
  memory was available to the app.

**Not confirmed:**

- **No game has been seen running on screen yet.** Nobody has watched a game appear. The cores
  are in the app and the files import, but "does it actually play" is still an open question.

Nothing below should be read as proof that games play. When you are ready to find out, the
checklist in [TESTING.md](TESTING.md) walks through it step by step.

The pictures further down came from the older browser prototype, not from the iPhone app. They
are in here to show what the design is aiming at, not to show the iPhone app working.

## How to get the app onto your phone

There is no Mac in this project. GitHub builds the app for you.

1. Open this repository on GitHub and go to the **Actions** tab.
2. In the list on the left, pick the workflow called **iOS**.
3. Press **Run workflow**.
4. Wait for the run to finish and go green. It compiles five emulator cores, so it is not quick.
5. Open the finished run and scroll to **Artifacts**.
6. Download the artifact named `Continuum-ipa-<commit>`, where `<commit>` is the short code for
   the version that was built.
7. That download is a zip. Inside it is `Continuum.ipa`.
8. Install `Continuum.ipa` with whichever sideloading tool you use. Sideloading just means
   installing an app that did not come from the App Store.

Most recent green build: run `35523953475`, artifact `Continuum-ipa-783e575`, about 5.16 MB.

### One thing to know about installers

The `.ipa` is signed with two special permissions:

- **JIT**, which lets the emulator translate game code as it runs. This is what makes the
  heavier consoles fast enough to be playable.
- **A raised memory limit**, about 6.8 GB instead of the few hundred MB an app normally gets.

Installers differ in whether they keep those permissions. TrollStore keeps them. Tools that
re-sign the app before installing it usually strip them out. The app should still install and
run either way, just with less headroom, which will matter later for the bigger consoles rather
than for the ones shipping today. If something goes wrong, tell me which installer you used, as
that changes what the likely cause is.

## How to add games

1. Open the app.
2. Tap **Import Games**.
3. Pick one or more game files.
4. **For a PlayStation game, select the `.cue` and every one of its `.bin` files in the same
   go.** In the file picker that means: tap **Select**, tap each file, then tap **Open**. If you
   import the `.cue` on its own the game cannot load, because the `.cue` only points at tracks
   that are not there.
5. The app copies the files into its own storage, so you can move or delete the originals
   afterwards.
6. Tap a game in the list to play it.

There is a second way in. The app's folder shows up in the iPhone **Files** app, under
**On My iPhone > Continuum**. Anything you drop in there appears in the app's list too, with no
importing needed.

## What it will look like

The browser prototype's look is the agreed design target for the iPhone app: a dark,
Netflix-style library with one big featured game at the top, rows of cover art with a small
system badge on each, and a tab bar along the bottom for Home, All Games, Favorites and
Settings.

| Library | Playing | Settings |
| --- | --- | --- |
| ![Library, browser prototype](docs/library-mobile.png) | ![Player, browser prototype](docs/mobile-player.png) | ![Settings, browser prototype](docs/settings-mobile.png) |

*All three images are the browser prototype, not the iPhone app.*

**The iPhone app does not look like this yet.** Right now it is a plain list of your games on a
black background, with a block of small diagnostic text above it. That text is there on purpose:
on an app installed this way there is no other way to see what went wrong. [TESTING.md](TESTING.md)
explains how to read it.

These four pictures are also from the browser prototype. They show four of the cores drawing
their own test cartridges, which is the closest thing to proof of emulation that exists so far.

| NES | Game Boy Advance | Master System | SNES |
| --- | --- | --- | --- |
| ![NES test cart in the browser prototype](docs/frame-nes.png) | ![GBA test cart in the browser prototype](docs/frame-gba.png) | ![Master System test cart in the browser prototype](docs/frame-sms.png) | ![SNES test cart in the browser prototype](docs/frame-snes.png) |

## What is coming next

More consoles, roughly easiest first:

1. **Nintendo 64**
2. **PSP**
3. **Nintendo DS**
4. **Nintendo 3DS**
5. **Switch**

None of these are built yet. The last two in particular are a long way off. There is a piece of
groundwork for Switch in `native/switch-wrapper/`, and its own tests pass, but there is no Switch
emulator behind it, so it does not play anything.

## What needs testing

[TESTING.md](TESTING.md) is a checklist you can work through on your phone. It says what to do,
what you should see if it worked, and what to send me if it did not. Start at the top: the first
test worth doing is a single-file cartridge game such as a `.nes` or a `.gba`, because those need
no extra files, so they answer "does emulation work at all" on their own.

## About the old web version

Everything under `web/` was a prototype that ran in a browser. It existed for one reason: to
prove the shared engine worked before there was any way to compile anything for an iPhone. That
job is finished.

It is not a product, it is not supported, and it is scheduled to be deleted. If you see it
described as a PWA or a web app anywhere in the older documents, that is history, not the plan.

## For developers and AI agents

The deep technical material lives in these documents, deliberately, so this page stays readable.

- [SESSION_HANDOFF.md](SESSION_HANDOFF.md) is the full engineering handoff: the architecture, the
  reasoning behind each subsystem, every trap already paid for, and the list of things not to
  undo. Read this first before changing code. Sections 16 and 17 cover the iOS build and the
  five-core `.ipa`.
- [docs/NATIVE_IOS_BLUEPRINT.md](docs/NATIVE_IOS_BLUEPRINT.md) is the design for the iOS app.
  Parts of it are now built.
- [docs/SET_HW_RENDER_DESIGN.md](docs/SET_HW_RENDER_DESIGN.md) is the graphics design for
  hardware-rendered cores, which is what N64 and everything above it will need.
- [.kiro/steering/product-scope.md](.kiro/steering/product-scope.md) states the scope in one
  place: the `.ipa` is the only deliverable, `web/` is legacy, and the browser UI is the design
  reference for the iOS UI.
- [CLAUDE.md](CLAUDE.md) holds the working conventions for agents in this repository.

The quick checks that run anywhere, including on Linux:

```bash
cargo test                                                    # 74 tests
cargo test --features native-core,uniffi-bindings             # 84 tests
cargo check --profile ios --target aarch64-apple-ios
scripts/build-core.sh ios-names                               # the five core filenames
```
