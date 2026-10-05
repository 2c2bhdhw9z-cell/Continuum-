# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> `HANDOFF.md`, then `STATUS.md` ("Next up" first) and `TESTING.md` section A. Then carry on.

## Where things are (5 October 2026)

- Newest install: build 124, release `build-124-d0df06a`, version 0.8.0 (124), 32 cores, checked.
  Link: https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-124-d0df06a/Continuum-124.ipa
- After that, commit `870b276` cleaned up the repo (docs, old web-era leftovers, comments). It
  starts build 125, which should be the same app as 124. Check that it built. If it failed, read
  the log, fix it and push. If it passed, check the .ipa (0.8.0 (125), 32 cores), but the owner
  does not need to install it unless something else changes.
- The owner is testing build 124 at home: TESTING.md section A (A1 to A7). Their answers decide
  what comes next. Anything that fails gets fixed first.
- Everything else that is planned is in STATUS.md "Next up" and docs/MANIC_PARITY.md "Still to do".

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
