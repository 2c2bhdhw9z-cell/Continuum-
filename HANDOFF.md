# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> `HANDOFF.md`, then `STATUS.md` ("Next up" first) and `TESTING.md` section A. Then carry on.

## Where things are (6 October 2026)

- Newest install: build 127, release `build-127-a611e43`, 0.8.0 (127), 32 cores, checked. Link:
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-127-a611e43/Continuum-127.ipa
- Build 127: the fixes from the owner's build 126 testing and the rebuilt feedback system
  (STATUS.md "Build 127"; TESTING.md A1 to A4). The owner has the link, not tested yet.
- Build 129 (pushed 6 October; 128 was cancelled for a wording pass): the app cleaned up for a
  public beta, every tester-facing line in a human voice (STATUS.md "Build 129";
  TESTING.md A1 to A5). The owner posts the .ipa straight into Telegram groups and Reddit, not the GitHub page, so
  testers only ever see the app itself. Feedback email:
  idkplswrk@gmail.com. Build 127's 3DS model change and cheat-off fixes are still untested.
- Everything else that is planned is in STATUS.md "Next up" and docs/MANIC_PARITY.md "Still to do".
- QA Wolf native iPhone testing was connected on 6 October: project MCP in
  `.kiro/settings/mcp.json`, and `.github/workflows/qawolf-mobile.yml` uploads the released
  `Continuum.ipa` (QA Wolf takes the .ipa directly, not a .app). The owner added the private
  GitHub Actions secret `QAWOLF_API_KEY`, and manual workflow run `37506142697` successfully
  uploaded build 127's `Continuum.ipa` to QA Wolf without starting a test or using credits. Still
  needs the owner to sign in to the MCP connection and have QA Wolf enable native iOS mobile
  testing/triggers for the workspace. Automatic test runs stay off until the repository variable
  `QAWOLF_MOBILE_TRIGGER` is `1`. The owner says QA Wolf support is not responding.
- Other remote testing tried and dropped (6 October): Momentic runs only on simulators, so it would
  need a whole simulator build (and 19 buildbot cores have no simulator version); the owner said
  not to. Apptest.ai rejected the .ipa ("The app file is not supported") because it carries no
  Apple certificate or provisioning profile. A built-in self-test was offered and turned down: the
  owner has already proved more than half the systems on the phone, so do not offer it again.

## Build 126 test results (6 October)

- Pass: 3DS saves load, an imported save keeps its picture, the small text flash and (i) with a
  skin, the in-game cheat box.
- Fail, fixed in 127: changing 3DS System Model crashed the app with no restart button; a
  walk-through-walls cheat froze a Pokemon GBA game (it was the FireRed v1.1 CodeBreaker code, and
  switching it off did nothing because mGBA ignores the off flag).
- The owner called the feedback form "basic": rebuilt in 127.
- Not answered yet: Jaguar speed, the 3DS stutter after transitions, Next disc (multi-disc PS1),
  and which game and version the owner's Pokemon file is (TESTING.md A2 step 2 asks).

## Build 125 test results (recorded in STATUS.md and TESTING.md)

Passed nearly everything; see STATUS.md and TESTING.md's "Already confirmed" table. The two
failures (3DS states refused as "too short", imported slots with no picture) were fixed in 126 and
confirmed.

## The owner's cheat question (answered 5 October)

They asked whether normal GameShark-type codes work. Yes: a typed code goes straight to the
system's emulator, which reads its own kinds (table in `crates/emulator-bridge/src/cheats/formats.rs`,
read off each core's source). Some emulators ignore typed codes entirely (3DS, Dreamcast, Atari
2600, arcade, Yabause, Amiga, C64, Lynx, 5200, Virtual Boy, PC Engine CD, Pokemon Mini). A code is
for one exact game version; the cheat screen names the version for GBA and Game Boy games.

## How the owner works

- They are not a programmer and use only an iPhone. Plain English, explain any technical word,
  and describe what they will see on the phone. Never ask them to read files, logs or GitHub.
- They test only the .ipa. After every build, check the .ipa and give the direct link without
  being asked, plus a short list of easy tests (they often test at work).
- When they are testing, fix nothing until they say "I'm done testing for now"; just record.
- Never re-ask a test they already covered, and when they ask "what do I test", give the full
  steps right there in the reply, not a pointer to an earlier message.
- Push straight to master: no branches, no pull requests. Save and push often, in big batches.
- No JIT, no web build. Behaviour goes in the Rust engine. Only Kiro works on this repo.

## How to check work before pushing (the sandbox can't build the iPhone app)

- `cargo test --workspace --features emulator-bridge/native-core` (606 tests)
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
- A wait-and-check command can time out while a build is still running; that is not the build
  failing. Check the run itself.
