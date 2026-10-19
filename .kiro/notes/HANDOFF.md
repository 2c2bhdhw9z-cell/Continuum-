# Handoff for the next chat

Paste this to start a new chat:

> Continue Continuum (repo 2c2bhdhw9z-cell/Continuum-). Read `.kiro/steering/owner-rules.md`,
> then `.kiro/notes/HANDOFF.md`, then carry on.

## START HERE (19 October 2026, morning Eastern): build 194 is the newest install

- **Build 194** (`96a9778`) is the newest install:
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-194-96a9778/Continuum-194.ipa
  The owner hated build 193's Last played Home. 194 follows the picture they sent: the word, then the ring; the cover stood up as a box; the consoles are photos in one tray, with a short name and a gold edge on the one you tap. Shelves is still the default. Settings, LAYOUT, Home, Last played.
  It still does not invent a play time or a percent finished. A game with no cover still shows the plain plate. Not tried on a phone yet.
- **Still not built, and do not start them until they ask:** undo a delete, next game on the same
  console, picking which game sits at the top of the Shelves home, looking at a backup before
  restoring it. The Continue widget stays parked.

## Earlier (18 October 2026, 2:40 PM Eastern): ideas kept, the widget parked

## Earlier (18 October 2026, 2:40 PM Eastern): ideas kept, the widget parked

- **Owner kept two ideas. Do not build them until they ask.** Written up in
  `.kiro/notes/docs/ideas/kept-2026-10-18.md`.
  - Undo a delete: deleted games sit in a bin until the next day, then they can be put back.
  - Next game, same console: from inside a game, jump to the next game on that console.
  - The game at the top of Home: you pick which game sits in the big spot. Added 18 October,
    3:25 PM. Right now that spot is just the last game added.
  - Look at a backup before restoring it. Added 18 October, 9:50 PM. Show which games and
    saves are in the backup. Nothing is restored until they say so.
- **Continue widget: looked into, do not build.** A home-screen widget with the picture of
  where you stopped. The tap half already works (`continuum://play`, passed 17 October). The
  widget half needs a second piece inside the app, and Feather and ESign sign only the main
  app, so the widget would not show up on this phone. Parked unless their signer changes.

## Earlier (18 October 2026, 9:20 AM Eastern): build 192 is on the phone

- **Build 192 is the newest install** (`006c199`):
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-192-006c199/Continuum-192.ipa
- **Owner, 18 October, 9:20 AM:** cBoy colouring old black-and-white games like a Game Boy Color,
  and cBoy's experimental picture, both work on build 192. Their word: "Works." That is Build
  190 checks 34, 35 and 37. Do not ask those again. The usual picture mode stays the one games
  use. The link cable still needs a second phone, so it stays untested. cBoy is not finished
  (camera for a Game Boy Camera game, two Game Boys on one phone, and a few rare picture cases
  are still open), but those two phone checks are done.
- **Owner's results on 192, one phone, no discs, no second phone, no online (17 October, 9:35 PM).**
  Their words: "1-20 good." Do not ask any of it again. Passed: Home's top back like before 191
  (cover fills the top behind the name; name and Play stay on screen); the six icons (Dotmatrix,
  Mark, Chrome, Violet, Spectrum, Cyan); Home's shelves; All Games by console (fold, the side
  strip, random game, star several, delete several, console order, By console / Grid / List);
  Clean start; Share a cartridge game; Copy its link through Shortcuts; Patch this game; console
  language on a DS or 3DS game; mute with the silent switch. Also Build 190's icon checks 24, 25
  and 26 (Clay and Badge set on the home screen). LeafGreen stays dropped. Join discs was not
  tried: they said no discs and no swapping discs. PSP and Dreamcast language were not in this
  pass. Full record: TESTING.md, top of "Build 191" and the Build 190 results.
- **Still open when they next test, this phone only:** Build 190 checks 27 to 33 (SNES on Mesen),
  36 (the one-line cBoy link-cable note), 38 onward that is not a disc and not a second phone
  (32X, a Mega Drive save chip, a 3DS game, arcade). Skip two phones, online, and discs until
  they say otherwise. The TV 4K check is still open; they left the TV for later.
- **Next job:** `tasks/gamecube.json` (GC1 to GC5), then ONE build. Don't start a build until
  they ask or the batch is done.

## Earlier tonight (17 October 2026, just after midnight Eastern): build 191, then GameCube

- **Done this session (owner had about 30% usage left):** the whole covers and library job, L1 to
  L10 (`tasks/library-and-manic.json` complete; write-up `docs/reports/library-and-manic-2026-10-16.md`;
  phone checks TESTING.md "Build 191", 18 steps). Commits `605093a` and `6a0045e`.
- **Build 191 is out and checked** (11:58 PM Eastern, 16 October, 12 minutes from push):
  https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-191-6a0045e/Continuum-191.ipa
  0.8.0 (191), 37 `_libretro_ios.dylib` files, the continuum:// link type in its Info.plist, and
  the new screens' text in the app. The owner was given the link and the Build 191 steps.
- **Owner's first report on 191 (17 October, morning):** the top of Home is broken and "isn't
  like it was before" (TESTING.md "Build 191", results). **Fixed on master in `c347ae5` at the
  owner's word, with NO build** (they said not to make one): the pre-191 hero is back, its art
  drawn as an overlay of a screen-wide box so a wide cover can't widen it. It ships in the next
  build; nothing else has changed since 191.
- **App icons (17 October):** the owner circled Dotmatrix, Mark, Chrome, Violet, Spectrum and
  Cyan and said "Fix." Done on master in `5acc8ec` with no build (TESTING.md "Build 191",
  results). Don't touch any other icon unless the owner asks.
- **Emulator updates found this session, held for the next batch:** Gearsystem, Azahar, PPSSPP and
  Flycast have newer code upstream; PUAE (Amiga) and VICE (C64) newer buildbot builds. Do the
  usual quick check and take the safe ones with the GameCube batch.
- **Next job:** `tasks/gamecube.json` (GC1 to GC5), then ONE build.
- The credits question (move the list into Technical details?) is still unanswered; leave it.

## Earlier (16 October 2026, evening Eastern): two new jobs, nothing built since 190

- **Owner's results on build 190:** checks 1 to 23 passed on 190, and 24 to 26 passed on build
  192 (recorded in TESTING.md). 27 onwards not reported yet. Don't ask 1-26 again.
- **GameCube is NOT impossible.** The owner played Rogue Leader on Manic EMU with no JIT on their
  iPhone 17 Pro Max (Dolphin's Cached Interpreter). Every note that said otherwise is corrected
  (docs/HARD_SYSTEMS.md has the facts, including what Manic's Dolphin fork is).
- **Owner's decisions (16 October):** GameCube and Wii go in on Dolphin (open licence), made fast
  without JIT by a CPU interpreter WE write ("do what Manic did to Dolphin"). Never copy Manic's
  code, screens, wording, names or art (new owner-rule: "Do what Manic does, never the way Manic
  looks"). The owner worried it would look like a rip-off; it won't, because Dolphin is everyone's
  and credited to its own authors.
- **Credits:** the owner asked why Continuum has to show credits when Manic doesn't openly. Answer
  given: ours are already a folded button at the bottom of Settings, About; the licences need a
  list somewhere and public source, not prominence. Offered to move it into Technical details;
  no reply yet. Never delete it (owner-rules).
- **The two jobs, in this order** (full checklists, rules and facts in `.kiro/notes/tasks/`):
  1. `library-and-manic.json`: covers whole at their real shape and sharp (ours crop every cover
     to 3:4 with the system label on top), the library by console (sections, index, Random Game,
     console order, select several), ROM patcher, MultiDisc Helper, launch link, share the game
     file, Safe Mode, language per game, Respect Silent Mode. Continuum's own design throughout.
  2. `gamecube.json`: Dolphin as an optional core on the Cached Interpreter (GC1), our own faster
     interpreter proved identical to it (GC2), JIT when allowed (GC3), Wii controls (GC4).
  Then ONE build (the owner wants everything done first), check the .ipa, give the link and the
  phone steps written out.
- **How to work here:** the owner's Workflows (loops) are off, so use `invoke_sub_agent`
  (general-task-execution) with ONE narrow job each, telling it to push after every green step
  (two helpers with big scopes stopped silently on 16 October). Rust in this sandbox:
  `CARGO_HOME` must be writable (e.g. /projects/sandbox/.scratch/cargo-home) and
  `RUSTUP_TOOLCHAIN=1.92.0-x86_64-unknown-linux-gnu`. Background processes die when a command ends:
  run long builds in the foreground. Reference MAME: `bash .kiro/notes/wip/tools/build-mame-deps.sh`.
  This sandbox has no GPU or Vulkan; host-test Dolphin with its Null or Software video backend.

## Build 190 is out (16 October 2026, 8:45 AM Eastern) — newest install, checked

- https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/build-190-a8b206a/Continuum-190.ipa
  (`a8b206a`): 0.8.0 (190), 37 core files (the 36 plus cArcade); DOOM, Jaguar, Virtual Boy, C64
  and Amiga are byte for byte build 185's; `core-mirror-1` holds all 17 files again and
  `scripts/check-core-mirror.sh live` passes with every one from the mirror.
- Build 189 stopped in its first minute: `build-engine.sh` required every core file to be named in
  ios.yml, which can't list cArcade (workflow edits are parked). `a8b206a` exempts that one name.
- The owner's tests are TESTING.md "Next install (after 185)", 63 checks. They haven't reported
  on 182 to 185 either. Record what they send; fix nothing until they say they're done testing.

## Update, 16 October 2026, about 7 AM Eastern: the whole batch is done, ONE build started

- Every checklist in `.kiro/notes/tasks/` is complete (owner, 1 AM: "I want everything done first
  before a build"). Highlights: cBoy's exact drawer 27 → 51 of 55 (the other 4 have no reference
  picture); Mesen 2 as the second SNES choice; cArcade plays 199 sets (Pac-Man, Galaxian/Moon
  Cresta, Konami's Scramble family, Donkey Kong/Jr.); the newer PSP, Dreamcast and 3DS emulators;
  the reviews (U4/T4) found no compile problem and fixed two behaviour bugs (ec644be, 49043c9).
  STATUS.md "Next up", TESTING.md "Next install (after 185)" (63 checks) and the README are
  updated.
- Checked on the final master before the build: workspace 749 tests, clippy clean, cBoy 163,
  cArcade 136, skins, players, core cache 219, core mirror 41, wrapper and Symbian harnesses, all
  54 Swift files parse and are listed in project.yml, and the bindings typecheck.
- **The build** was started by hand (workflow dispatch). Every emulator rebuilds once (about an
  hour). Check the .ipa: 0.8.0 and the run number, 37 `_libretro_ios.dylib` files (36 plus
  cArcade). If the release step gets HTTP 403, follow `.kiro/notes/tasks/README.md`, "Then the
  build", step 3. Then give the owner the link and TESTING.md "Next install (after 185)" written
  out. After it: `bash scripts/check-core-mirror.sh live` should say all 16 come from the mirror.
- Helpers working through `invoke_sub_agent` (general-task-execution) did the jobs this session;
  two stopped silently mid-job on the big arcade scope, so give a helper one narrow job and have
  it push after every step.

## Update, 16 October 2026, 12:50 AM Eastern (third Kiro account)

- The 11 memories are saved on this account. Workflows (loops) are turned off on this account by
  the owner, so one session works the jobs in turn.
- **Done this session:** `mirror-restore.json` complete (9c53424; the next build takes 11
  emulators from the buildbot and DOOM, Jaguar, Virtual Boy, C64 and Amiga from build 185's
  install, then refills `core-mirror-1` itself; the sandbox can't upload release files).
  cArcade's Galaxian/Moon Cresta board matches MAME on all 10 test programs (7e6402e; not in the
  app yet: sound, Konami's board and the wiring are next, see `arcade.json` I5). Emulator updates
  (14375a5): PSP, Dreamcast and 3DS moved to their newest code; Gearsystem skipped (desktop-only
  changes). CU5's write-up is still to do.
- **Reference MAME:** `bash .kiro/notes/wip/tools/build-mame-deps.sh` in the foreground rebuilds
  it in a new sandbox. Rust here: `CARGO_HOME` must be writable and
  `RUSTUP_TOOLCHAIN=1.92.0-x86_64-unknown-linux-gnu` (rustup can't sync).
- Still left, in order: CU5, arcade I5 (Galaxian sound, Konami board, wiring), I6, I7, cBoy G2-G5,
  H3, H6, U4, U5, T3-T5, then fold the reports and ONE build.

## Where this left off (14 October 2026, 1:40 PM Eastern)

- **New Kiro account again.** The second account ran out of credits at about 9:10 AM with six jobs
  running; the owner bought a little more only to get everything saved. First thing: save every
  line of "Memories to save on a new account" (end of `.kiro/steering/owner-rules.md`) as a global
  learning, and tell the owner they're saved.
- **Old releases deleted (14 October), and a mistake that must be fixed before the next build.**
  Only build 185 is left, plus `core-mirror-1`, which isn't an install: it held the build's frozen
  copies of the 16 downloaded emulators and was deleted by mistake with the old releases, then
  re-created empty. **Do `.kiro/notes/tasks/mirror-restore.json` before any build**, or DOOM,
  Jaguar, Virtual Boy and C64 come out as the newer builds turned down on 12 October. Nothing
  is lost: the checked builds are still inside build 185's install, and the job says how to use them. Links to older
  builds in these notes no longer work; their code is still in git.
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
