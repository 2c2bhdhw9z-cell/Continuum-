# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> `HANDOFF.md`, then `STATUS.md` ("Next up" first) and `TESTING.md` section A. Then carry on.

## Where things are (5 October 2026)

- Build 126 is the fix batch from the owner's build 125 testing, pushed 5 October. STATUS.md
  "Build 126" lists it; TESTING.md A1 to A6 are its tests. Check the Releases page for whether it
  finished, and check the .ipa before handing it over.
- Previous install: build 125, release `build-125-870b276`, 0.8.0 (125), 32 cores.
- **Waiting on the owner for one thing:** the email address the feedback form should open Mail
  with. It goes in `FeedbackDestination.email` in `native/ios/Feedback.swift` (one line), then a
  build. Until then the form opens the share menu, which works.
- Everything else that is planned is in STATUS.md "Next up" and docs/MANIC_PARITY.md "Still to do".

## Build 125 test results (all recorded in STATUS.md and TESTING.md already)

- Passed: save slot pictures, save and load on 14 systems (NES, SNES, GB, GBC, GBA, Game Gear,
  Mega Drive, PS1, DS, TurboGrafx-16, Atari 2600, N64, Jaguar, Pokemon Mini), five new systems
  running (GB, TurboGrafx-16, Atari 2600, Jaguar, Pokemon Mini), the Apple overlay switch, restart
  from Core settings, cover lookups off, the login button and an unlock banner, paste, Open in,
  zip, the sharper cover, battery saves, rename / save over / delete, export and import on a
  non-3DS game, keyboard / tilt / shake, 3x / 4x / slow motion, haptics and rumble, pad hiding
  with a controller, LCD grid and dot matrix, palette and rotate, swap screens and the six
  layouts, the game on a TV.
- Failed, fixed in 126: every 3DS state refused as "too short"; imported slots had no picture.
  Also fixed in 126 without a report: the small text never showing with a screen-hole skin, and
  (i) doing nothing there.
- Not answered yet: Jaguar speed, the 3DS stutter after transitions, Next disc (multi-disc PS1).
- The owner was angry at being asked again about tests already covered. Never re-ask; work it out
  from what they sent.
- The owner asked for test lists with nothing needing a computer or a second phone: B1, B7 and H1
  sit in their own skip section at the end of TESTING.md's list.

## The owner's cheat question (answered 5 October)

They asked whether normal GameShark-type codes work. Yes: a typed code goes straight to the
system's emulator, which reads its own kinds (table in `crates/emulator-bridge/src/cheats/formats.rs`,
read off each core's source). Some emulators ignore typed codes entirely (3DS, Dreamcast, Atari
2600, arcade, Yabause, Amiga, C64, Lynx, 5200, Virtual Boy, PC Engine CD, Pokemon Mini); the RAM
search still works there when the game's memory is readable.

## How the owner works

- They are not a programmer and use only an iPhone. Plain English, explain any technical word,
  and describe what they will see on the phone. Never ask them to read files, logs or GitHub.
- They test only the .ipa. After every build, check the .ipa and give the direct link without
  being asked, plus a short list of easy tests (they often test at work).
- When they are testing, fix nothing until they say "I'm done testing for now"; just record.
- Push straight to master: no branches, no pull requests. Save and push often, in big batches.
- No JIT, no web build. Behaviour goes in the Rust engine. Only Kiro works on this repo.

## How to check work before pushing (the sandbox can't build the iPhone app)

- `cargo test --workspace --features emulator-bridge/native-core` (598 tests)
- `cargo clippy --workspace --features emulator-bridge/native-core`
- `bash scripts/check-skins.sh`, `bash scripts/check-players.sh`
- `bash scripts/fetch-libretro-headers.sh && bash native/switch-wrapper/build.sh host` (15/15)
- `swiftc -frontend -parse <file>` on each changed Swift file. This only checks the writing, not
  the types, so a Swift type mistake shows up only on the build server.
- A new Swift file must be listed in `native/ios/project.yml` `sources`, or it is silently skipped.
- Bindings: `cargo build --release --features emulator-bridge/native-core,emulator-bridge/uniffi-bindings -p emulator-bridge`,
  then `./target/release/uniffi-bindgen generate --library target/release/libemulator_bridge.so --language swift --out-dir <dir> --no-format`,
  rename the modulemap to `module.modulemap`, and `swiftc -typecheck -I <dir> <dir>/*.swift`.
- Builds: `gh api "repos/2c2bhdhw9z-cell/Continuum-/actions/runs?per_page=3"`. Releases:
  `gh api repos/2c2bhdhw9z-cell/Continuum-/releases/latest`. Check an .ipa: count
  `_libretro_ios.dylib` files (32) and read `CFBundleShortVersionString` / `CFBundleVersion` from
  `Payload/Continuum.app/Info.plist`.
