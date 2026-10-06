# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> `HANDOFF.md`, then `STATUS.md` ("Next up" first) and `TESTING.md` section A. Then carry on.

## Where things are (5 October 2026)

- Newest install: build 125, release `build-125-870b276`, version 0.8.0 (125), 32 cores, checked.
  Link: https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-125-870b276/Continuum-125.ipa
- Build 125 is build 124's app (the bug sweep, commit `d0df06a`) plus a repo cleanup (commit
  `870b276`: docs, old web-era leftovers, comments). Inside the app only the local-network
  permission sentence changed.
- **The owner is testing build 125**, working through the whole TESTING.md list (section A
  first). Their answers come back as test numbers. Anything that fails gets fixed first.
- Everything else that is planned is in STATUS.md "Next up" and docs/MANIC_PARITY.md "Still to do".
- **The owner said: fix nothing until they say "I'm done testing for now".** Collect results here.

## Build 125 results so far (5 October, Mario Kart 7)

- A1 passes on a 3DS slot: Slot 1 shows the game's picture, tall, both screens.
- A2: Export and Import a state file work (Slot 2, imported, azahar 065c922). Loading it fails:
  "state too short: it is 14627226 bytes and the core needs at least 19231622", then 17504282 on
  the next try. Azahar's reported state size moves, so the "too short" check in
  `SaveStates.refusal(for:)` (native/ios/SaveStates.swift) wrongly refuses real 3DS states. It
  probably blocks every 3DS state, the original slot and the auto-save resume too (owner asked to
  try loading Slot 1).
- Fix after testing ends, with: show the status line briefly with a skin that has screen holes,
  make (i) work with such a skin, Apple overlay hide via `"mode": "disabled"`, and a picture for
  imported slots.

## How the owner works

- They are not a programmer and use only an iPhone. Plain English, explain any technical word,
  and describe what they will see on the phone. Never ask them to read files, logs or GitHub.
- They test only the .ipa. After every build, check the .ipa and give the direct link without
  being asked, plus a short list of easy tests (they often test at work).
- Push straight to master: no branches, no pull requests. Save and push often, in big batches.
- No JIT, no web build. Behaviour goes in the Rust engine. Only Kiro works on this repo.

## How to check work before pushing (the sandbox can't build the iPhone app)

- `cargo test --workspace --features emulator-bridge/native-core` (590 tests)
- `cargo clippy --workspace --features emulator-bridge/native-core`
- `bash scripts/check-skins.sh`, `bash scripts/check-players.sh`
- `bash scripts/fetch-libretro-headers.sh && bash native/switch-wrapper/build.sh host` (15/15)
- `swiftc -frontend -parse <file>` on each changed Swift file. This only checks the writing, not
  the types, so a Swift type mistake shows up only on the build server.
- Bindings: `cargo build --release --features emulator-bridge/native-core,emulator-bridge/uniffi-bindings -p emulator-bridge`,
  then `./target/release/uniffi-bindgen generate --library target/release/libemulator_bridge.so --language swift --out-dir <dir> --no-format`,
  rename the modulemap to `module.modulemap`, and `swiftc -typecheck -I <dir> <dir>/*.swift`.
- Builds: `gh api "repos/2c2bhdhw9z-cell/Continuum-/actions/runs?per_page=3"`. Releases:
  `gh api repos/2c2bhdhw9z-cell/Continuum-/releases/latest`. Check an .ipa: count
  `_libretro_ios.dylib` files (32) and read `CFBundleShortVersionString` / `CFBundleVersion` from
  `Payload/Continuum.app/Info.plist`.
