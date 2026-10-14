# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> then `.kiro/notes/HANDOFF.md`, then carry on.

## Where this left off (14 October 2026, 1:40 PM Eastern)

- **New Kiro account again.** The second account ran out of credits at about 9:10 AM with six jobs
  running; the owner bought a little more only to get everything saved. First thing: save every
  line of "Memories to save on a new account" (end of `.kiro/steering/owner-rules.md`) as a global
  learning, and tell the owner they're saved.
- **Old releases deleted (14 October):** only build 185 is left on the Releases page, with its tag
  (owner's choice: nothing was public yet). Links to older builds in these notes no longer work;
  their code is still in git.
- **Newest published install: build 185** (`393f12c`, 13 October), checked: 0.8.0 (185), 36 core
  files: https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-185-393f12c/Continuum-185.ipa
  It has the night batch (TESTING.md "Night batch (13 October)"). The owner hasn't reported on 182
  to 185 yet. **Build 188** was started at 9:01 AM without the owner's yes and cancelled at 9:04 in
  its first checks. Nothing has been built since 185, and no build starts until the batch is done or
  the owner asks (owner-rules).
- **The batch is in `.kiro/notes/tasks/`.** Its README has the table of what's done and left, how to
  run the jobs, and the build steps. The two items cut off mid-way are saved as patches in
  `.kiro/notes/wip/` (README there). Nothing was left only in the sandbox.
- **On master since 185, not built yet:** the two-phone Game Boy link (app half); cBoy colouring
  black-and-white games like a Game Boy Color (off by default; Mealybug 27 of 55), its Link cable
  text and one-line setting descriptions; arcade games back on our own emulator, cArcade (65 Pac-Man
  board sets, matching MAME frame for frame; its Z80, 8035 and AY-3-8910 proven); 32X games start
  with no BIOS files (our own start-up code: two open-licence 32X games reach their menus on a host
  build); Sega CD multi-disc playlists, Next disc and the save cartridge; every Mega Drive save-chip
  and J-Cart cartridge in MAME's list recognised; newer Gearsystem (Master System, Game Gear) and
  Azahar (3DS: old states won't load, and the heads-up says so); light mode (Settings, Appearance:
  Dark, the default, Light, System); the "80 games" strip gone (its messages pop up and fade, the
  count is in Settings, Storage); Settings, Layout, Game card size; the floating tabs; 10 of the 12
  circled app icons fixed (Cyan and Violet would need new art: the owner's call) and the Default
  tile; shorter text in all of Settings (the PSP section is gone) and most other screens; the build
  cache's second pass (comments, the cache's own code and the emulator lists rebuild nothing; 213
  checks); a friendlier README and a tidier docs folder.
- **Left** (full instructions in each checklist): cBoy's exact drawer, G2 to G5 (go-ahead given; the
  owner's thinking is always at max, so never ask again); arcade I5 (half done, in the WIP patch),
  I6 Donkey Kong, I7; Mesen 2 as a second SNES choice (H3) and H6; the PSP and Dreamcast updates
  (CU3, CU4) and CU5; the Home screen review and write-up (U4, U5); the rest of the shorter text (T3
  to T5). Then fold the reports into STATUS.md and TESTING.md, ONE build, check the .ipa (37 core
  files: the 36 plus cArcade), and give the link and the phone steps in the same reply. Repo cleanup part 2 (the notes off the front page) is done; only the
  About box and the release-notes text are left, both blocked (tasks README, "Parked"); after the next build, update the README's list of
  systems (`.kiro/notes/tasks/repo-cleanup-phase2.md` section 9).
- **Tools:** every account has the same tools (the owner was right; the first session on the second
  account missed the loop tool for hours). Jobs run as `bundled://ralph` loops, one per job, all started at once. Fetching anyone else's repository needs `GIT_CONFIG_GLOBAL=/dev/null` in front (the
  sandbox's git sends github.com through a gateway that only serves this repository). The GitHub
  token can't edit the repo's About box (403), and Kiro's rules allow no workflow-file change
  without a pull request, so those wait (tasks README, "Parked").
- **What went wrong on 14 October, so it doesn't again:** a Kiro restart paused three loops for half an hour and
  nobody resumed them until the owner wrote (check every loop whenever you wake); the owner was told
  they couldn't see the jobs, but their app has a Workflows tab (this chat's loops didn't appear in
  it, reason unknown, so post a one-line update in the chat whenever a piece lands); the owner was
  asked about max thinking again (it's always at max); and build 188 was started without a yes.

Older, kept as history: build 144 (`build-144-74050e5`) passed the owner's four phone checks on
9 October (Home readable after the big cover, Home and All Games scroll, re-importing keeps a
game, fast-forward lets go after Control Centre, the (i) line names the phone). Do not ask those
again. Not shown, so not Done: rewind letting go the same way, covers not flashing (TESTING A8),
the phone name on the feedback screen (A9), one bad skin not wiping the list, and a random skin
id not becoming a console. A skin that does not name itself can still show up twice across two
phones. No JIT test.

## Check for newer emulator versions at the start of every session (owner ⭐, 12 October 2026)

The owner wants the newest version of every emulator, always, as long as it does not break
anything major compared with the version we ship. Not a robot doing it: Kiro checks every
session and takes them itself, after checking thoroughly. The full rule is in
`.kiro/steering/owner-rules.md` ("Emulator updates"). In short:

1. Every session, check every core for a newer version. That means the 19 pinned builds from
   source (`bash scripts/build-core.sh ios-pins`: 14 required cores, Dreamcast, Symbian and the
   3 JIT builds) AND the 16 downloaded ones (sha-pinned in `scripts/fetch-buildbot-cores.sh`).
   `scripts/check-core-updates.sh` covers both: it writes `docs/CORE_UPDATES.md` with what's
   newer, the last few upstream changes, whether our patches still fit, and the list of updates
   already checked and skipped.
2. For each one that is newer, check it thoroughly before taking it: read the upstream changes
   for anything removed or broken (systems, options, iOS support, save formats), make sure our
   patches still apply, build it, run every check, and check whether old save states and
   in-game saves still load. Anything that would break, take out, or slow down something
   players already have counts as major: then do not take it, and tell the owner why in
   plain words.
3. Take the good ones in the next batch (one build), and tell the owner in plain words which
   consoles got newer emulators and what changed for them.
4. Players get a one-time heads-up in the app for each updated emulator (built on master,
   13 October: `crates/emulator-bridge/src/core_updates.rs`). See "Players get a heads-up" in
   owner-rules.md for the wording and the rules.

Moving a pin: follow "MOVING A PIN" in `scripts/build-core.sh` (and "Moving a pin" in
`scripts/fetch-buildbot-cores.sh`, which needs the zip pin and the dylib pin). Also update
`crates/emulator-bridge/src/core_updates.rs` (the version it ships, and an entry for the update,
saying whether old states still load): a test fails until it matches the scripts.

The weekly scheduled job (`.github/workflows/core-updates.yml`) has not run on GitHub yet (0
runs on 13 October). The script was run here on 13 October: all 32 sources up to date (27 on
their newest version, 5 newer ones already checked and skipped). Its page is only a hint, never
the whole answer.

## Older notes (7 October 2026, history)

- (Out of date: since build 142 the app has the owner's icon, and build 145 added 55 home screen
  icons. The steps in this bullet are still how an icon is put in.)
  **The app ships no icon, and that is the current state rather than an oversight.** Build 134 added
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
  testers only ever see the app itself. Feedback email (since 12 October 2026):
  ContinuumDev2c2b@gmail.com. Build 127's 3DS model change and cheat-off fixes are still untested.
- Everything else that is planned is in STATUS.md "Next up" and .kiro/notes/docs/MANIC_PARITY.md "Still to do".
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
  touching only `.kiro/notes/` or `docs/` (but not `docs/samples/`,
  which the skin check uses) never needs `[skip ci]`.
- **A build takes about 6-8 minutes on the Mac when the cores are cached, and about 45-50
  minutes when they are not.** The cores built from source are cached between runs (STATUS.md
  "Build 136"). Three things to know: each core is judged on its own inputs, so editing one
  `scripts/patches/<family>-*.patch`, or one core's own recipe inside the `# recipe-start:` region
  for it in `scripts/build-core.sh`, rebuilds that core (and the JIT build sharing its source) and
  leaves the rest cached — while editing the SHARED part of `scripts/build-core.sh` (a helper every
  core goes through, the staging, the common flags), or `native/continuum-symbian/` or
  `native/ios/build-engine.sh`, or a new Xcode on the runner, still rebuilds every core and pays
  the full 45-50 minutes, which is correct but worth saying first; cBoy
  (`cores/continuum-gb`) is NOT in the key since the 12 October fix and is compiled from source
  on every build, so a cBoy-only change costs about a minute, never a full rebuild (builds 177
  and 180 took about 47 minutes only because cBoy was still in the key). A cBoy-only push does
  start a build by itself now, so use `[skip ci]` until the last push of a batch. And the cache
  is saved in its own step straight after the cores are built, before the app is packaged, so a
  build that fails later (Xcode, packaging, the release) does NOT cost the owner the core build
  again on the retry. That step is deliberately not `if: always()`: a core
  build stopped part-way can leave a half-written core that the next build would trust.
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

- `cargo test --workspace --features emulator-bridge/native-core` (723 tests on 14 October)
- `cargo clippy --workspace --features emulator-bridge/native-core` (CI runs it too; an error
  stops the build)
- cBoy: `cd cores/continuum-gb && scripts/fetch-test-roms.sh && cargo test --release` (158 tests on 14 October).
  Run it before any cBoy push: builds 178 and 179 failed on these alone.
- `bash scripts/check-skins.sh`, `bash scripts/check-players.sh`
- `bash scripts/fetch-libretro-headers.sh && bash native/switch-wrapper/build.sh host` (15/15)
- `bash scripts/check-core-cache.sh` (213 checks; the per-core build cache)
- cArcade: `cd cores/continuum-arcade && cargo test --release`. CI doesn't run it yet (the workflow edit is parked), so run it before every cArcade push.
- `swiftc -frontend -parse <file>` on each changed Swift file. This only checks the writing, not
  the types, so a Swift type mistake shows up only on the build server.
- A new Swift file must be listed in `native/ios/project.yml` `sources`, or it is silently skipped.
- Bindings: `cargo build --release --features emulator-bridge/native-core,emulator-bridge/uniffi-bindings -p emulator-bridge`,
  `cargo build --release -p continuum-uniffi-bindgen` (the package's name; its binary is
  `uniffi-bindgen`), then `./target/release/uniffi-bindgen generate --library target/release/libemulator_bridge.so --language swift --out-dir <dir> --no-format`,
  rename the modulemap to `module.modulemap`, and `swiftc -typecheck -I <dir> <dir>/*.swift`.
- Builds: `gh api "repos/2c2bhdhw9z-cell/Continuum-/actions/runs?per_page=3"`. Releases:
  `gh api repos/2c2bhdhw9z-cell/Continuum-/releases/latest`. Check an .ipa: count
  `_libretro_ios.dylib` files (36 from the licence-swap install on: 33 cores plus 3 JIT builds;
  build 180 had 38) and read `CFBundleShortVersionString` / `CFBundleVersion` from
  `Payload/Continuum.app/Info.plist`.
- A wait-and-check command can time out while a build is still running; that is not the build
  failing. Check the run itself.
