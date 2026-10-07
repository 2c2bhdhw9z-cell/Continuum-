# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> `HANDOFF.md`, then `STATUS.md` ("Next up" first), `docs/CORE_UPDATES.md` and `TESTING.md`
> section A. Then carry on.

## Read docs/CORE_UPDATES.md at the start of every session

It is written weekly by a scheduled job and says which emulators their own authors have updated
since the version Continuum is locked to. **The owner is not tracking any of that and has said
plainly that they refuse to.** They will never open that file either, so it is only useful if
you read it and TELL them, in plain English, in a reply: which consoles have newer code, roughly
how far behind we are, and whether it looks worth taking. Then let them decide. Taking an update
is a deliberate edit to a pin in `scripts/build-core.sh`, a build, and a test on a phone — never
automatic, and never without saying so first.

As of 7 October 2026 the first run found 5 of 13 behind: PSP (132 commits), N64 (29),
Beetle PSX HW (14), 3DS (6), Dreamcast (5).

## Where things are (7 October 2026)

- **The app ships no icon, and that is the current state rather than an oversight.** Build 134 added
  a drawn one; the owner had asked to see artwork before it went into the app, it went in before
  they saw it, and they then rejected it along with three alternatives, so build 135 took it out.
  Do not put one back without them approving the picture first. To put one in:
  `python3 scripts/make-app-icon.py --from <picture>` (fits any picture to all 13 sizes, reads the
  real format from the bytes, flattens transparency, centre-crops), then restore the two lines in
  `native/ios/project.yml` the long comment there names. Details in STATUS.md "Build 135".
  The keyless image service is a dead end: watermarked output, see STATUS.md.
- Build 133, release `build-133-0e9a7e8`, 0.8.0 (133), 35 core dylibs (32 cores
  plus 3 JIT builds), checked. The app exports `_continuum_jit_region` / `_continuum_jit_release`,
  and ppsspp, azahar, pcsx_rearmed_jit and flycast_jit all have the lookup compiled in. Link:
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-133-0e9a7e8/Continuum-133.ipa
- Build 133 = JIT parts 1 and 2 (below): current iPhones included. Owner tests are TESTING.md J1
  and J2; build 129's A1 to A5 are still unanswered.
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

## JIT (owner, 7 October: required for everyone who can enable it, never required to work)

Build 131 (part 1, done in code, untested: the owner's phone cannot use JIT):
- `crates/emulator-bridge/src/jit.rs` reads `CS_GET_TASK_ALLOW` / `CS_DEBUGGED` (via
  `jit_probe::ios_aarch64::signing_status`) and the phone model and iOS version (`sysctl`
  `hw.machine`, `kern.osproductversion`). JIT is usable when debugged, not on a TXM phone (iOS 26,
  iPhone14,2+ / iPad14,5+, StikDebug's table), and the user switch is on. No `brk` is ever run.
- PPSSPP and Azahar: host answers `GET_JIT_CAPABLE` (74), frozen per core load; PPSSPP option
  defaults to "JIT" then (`options.rs` `host_rules_for`); Azahar patch
  `scripts/patches/azahar-use-jit-when-the-host-allows-it.patch` uses its own `CanUseJIT()`.
- PCSX ReARMed, parallel-n64, flycast: optional `_jit_` builds in `build-core.sh`
  (`IOS_OPTIONAL_CORES`), embedded in `project.yml`; Swift asks `engine.coreLibraryFor` in
  `ensureCoreLoaded`. Patches `pcsx_rearmed-ios-jit.patch` (run-time cache mapping, 16K pages)
  and `parallel_n64-ios-jit.patch` (mprotect instead of `pthread_jit_write_protect_np`).
- Settings, Technical details: JIT sentence, "Use JIT when it's available", StikDebug button
  (`stikdebug://enable-jit?bundle-id=&pid=`, no script, only when not TXM). `JIT:` line in (i)
  and feedback.
- All three `_jit_` builds compiled in build 131. If a later log shows one failing, fix it (it only
  warns; the .ipa still ships without it).

**Build 138: the N64 now gets JIT on current iPhones too, so every system with a JIT build has it
on every phone that can switch it on.** The reason it was listed as blocked was wrong twice over:
parallel-n64's trampoline allocator is compiled but never called at the pinned commit (nothing
calls `trampoline_init`), and its recompiler already has the two-address mode the Switch port uses,
with all ~38 write-address-to-run-address translations already written. It only needed the host's
pair wired into `base_addr` / `base_addr_rx`. See STATUS.md "Build 138". Untested on a phone, like
all the JIT work.

Part 2 (build 133, done): `crates/emulator-bridge/src/jit26.c` reserves a 512 MB region, has the
JIT app bless it (`JIT26PrepareRegion` / `JIT26Detach`, `brk #0xf00d`, only after `CS_DEBUGGED` and
only when the app itself asked for `universal.js`), and `vm_remap`s a writable view. Cores dlsym
`continuum_jit_region` / `continuum_jit_release`. Patches: `ppsspp-ios-jit-region.patch` (CodeBlock
takes the pair, `PlatformIsWXExclusive` then false), `azahar-ios-jit-region.patch` (oaknut CodeBlock
gains `wptr_base()`, dynarmic's `address_space.cpp` passes it, dynarmic's A32 `code_cache_size`
32 MB on iOS), `pcsx_rearmed-ios-jit.patch` (`TC_WRITE_OFFSET` + region, cache flush translates the
write address to the run address), `flycast-ios-jit-region.patch` (`FEAT_NO_RWX_PAGES` for the iOS
jit build, `prepare_jit_block` two-address overload from the region, the three `JITWriteProtect`
helpers become no-ops). `jit::core_may_use_jit` is the gate; the host hands out the same pair on
non-TXM phones so there is one path. parallel-n64 joined this in build 138 (above).

melonDS JIT is macOS-only code; DS is fine on the interpreter.

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
- Push straight to master: no branches, no pull requests.
- **ONE BUILD PER BATCH, NEVER ONE BUILD PER FIX.** The owner is angry about a session that left
  six builds in the Actions list, three of them cancelled because a later push superseded them.
  That looked like builds piling up in parallel; they were not. `ios.yml` already has a
  concurrency group (one running, at most one queued), so overlapping builds are almost always
  too many pushes rather than a concurrency fault — check `run_started_at` before claiming
  otherwise, and do not "fix" the workflow for what is a pushing-discipline problem.
- **How to satisfy that AND the rule about never losing work: `[skip ci]`.** The two pull in
  opposite directions — work must be pushed as soon as it is finished, because a session can end
  at any moment, but every push to a watched path starts a build. So: push each finished piece as
  it is done with `[skip ci]` in the commit message, then make the LAST push of the batch without
  it, and that single build contains everything. Nothing is ever left only in the sandbox, and the
  owner gets one build.
- Only the paths listed in `ios.yml`'s `on: push: paths:` start a build at all, so a commit
  touching only `STATUS.md`, `HANDOFF.md`, `TESTING.md` or `docs/` never needs `[skip ci]`.
- **A build takes about 6 minutes, not 40, as of build 136.** The 35 cores are cached between
  runs (STATUS.md "Build 136"). Two things to know: editing anything in `scripts/patches/` or
  `scripts/build-core.sh` changes the cache key and the next build pays the full ~40 minutes
  rebuilding everything, which is correct but worth saying before a one-line patch edit; and the
  cache is saved with `if: always()` right after the cores are built, so a build that fails later
  does NOT cost the owner the 35 minutes again on the retry.
- **Never open an image whose bytes you have not checked.** On 7 October a picture downloaded from
  an image service was named `.png` but was really a JPEG inside. Looking at it put a file in the
  chat's history whose declared type did not match its content, and from then on EVERY message the
  owner sent was rejected in about a second, before it was ever read — the chat was dead and a new
  one was the only cure, which cost a session and a lot of the owner's patience. Run `file` on an
  image, or convert it, before looking at it. Better still, draw artwork with a script (as
  `scripts/make-app-icon.py` does) so the bytes are never in doubt.
- JIT when a user can enable it, never required; no web build. Behaviour goes in the Rust
  engine. Only Kiro works on this repo.

## How to check work before pushing (the sandbox can't build the iPhone app)

- `cargo test --workspace --features emulator-bridge/native-core` (611 tests)
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
