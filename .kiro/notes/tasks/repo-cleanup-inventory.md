# Cleanup inventory (part 1, item R1)

Taken 13 October 2026 at 6:08 PM Eastern, against master `381d5d7` (this copy was up to date with
origin). Nothing tracked was changed.

Scope: the 81 tracked files outside cores/, crates/, native/ and scripts/: 9 at the root, 2 in
.github/, 1 in .kiro/ and 69 in docs/. scripts/ has no .md files.

Homes:
- **Public**: stays where it is.
- **Public, human pass**: stays, and gets rewritten for visitors (the item is named).
- **Tucked away**: already off the front page, so it stays.
- **Notes (part 2)**: moves to the working-notes folder, proposed `.kiro/notes/`.
- **Delete (R4)**: nothing uses it, so part 1 deletes it.
- **Delete (part 2)**: also unused, but it sits in a folder part 1 may not touch.

"Writer tonight" comes from the other jobs' checklists, which I only read
(`/projects/sandbox/.scratch/*/.agents/tasks/*.json`). Six are active: arcade (arcade-work),
build cache (cache-work), cBoy (cboy-work), icons (icons-work), shorter text (text-work) and Home
screen (ui-work). The arcade, cache, cBoy, text and Home screen jobs are all told not to edit
STATUS.md, TESTING.md or HANDOFF.md, because the orchestrator writes those at the end of the night.

## At a glance

| Home | Files |
| --- | --- |
| Public | LICENSE, Cargo.toml, Cargo.lock, rust-toolchain.toml, .gitignore, .github/workflows/core-updates.yml, docs/samples/ (5), docs/app-icon-source.png, docs/app-icon.png |
| Public, human pass | README.md (R2), the release-notes text in .github/workflows/ios.yml (R5), docs/DELTA_SKIN_FORMAT.md (R6) |
| Tucked away | .kiro/steering/owner-rules.md (R4 changes its archive line; part 2 changes its paths) |
| Notes (part 2) | HANDOFF.md, STATUS.md, TESTING.md; in docs/: PRODUCT_SCOPE, MANIC_PARITY, PLATFORM_LIMITS, HARD_SYSTEMS, NATIVE_IOS_BLUEPRINT, SET_HW_RENDER_DESIGN, CORE_OPTIONS_STANDARD, CORE_UPDATES, BUG_AUDIT, REVIEW_COVERAGE, ARCADE_CORE and the four CBOY_* files (all .md); docs/option-sources/ (24); docs/reports/ (5 today, with 3 more due tonight; one of the 5 is deleted instead); docs/ideas/ (1); docs/side-projects/ (5) |
| Delete (R4) | the 7 old screenshots, docs/archive/ (2), docs/HISTORY.md |
| Delete (part 2) | docs/reports/yellow-gb-177.txt |

## Root

| File | What it is | Referenced from | Runs, builds or ships? | Writer tonight | Home |
| --- | --- | --- | --- | --- | --- |
| README.md | The front page | Links out on lines 60-67 (STATUS.md, TESTING.md, docs/PRODUCT_SCOPE.md, docs/MANIC_PARITY.md, LICENSE). TESTING.md:758 links to it. docs/ARCADE_CORE.md:234 names its arcade line (line 30). Mentioned in owner-rules.md:85, HANDOFF.md:75, STATUS.md:1115 | No (GitHub displays it) | Nobody. The arcade plan leaves the arcade line "for whoever writes the notes" | Public, human pass (R2) |
| LICENSE | The GPL-3.0 text. GitHub already reads it: the API reports GPL-3.0 | README.md:67 | No build use | Nobody | Public |
| Cargo.toml | The Rust workspace | It's an ios.yml push path, and the build scripts use it | Builds | Nobody. The arcade crate stands alone, like cBoy | Public |
| Cargo.lock | Pinned crate versions | It's an ios.yml push path, and both cargo cache keys use it (ios.yml:108, :194) | Builds | Nobody | Public |
| rust-toolchain.toml | Pins the compiler at 1.92.0 | It's an ios.yml push path, both cargo cache keys use it, and build-core.sh:1010 mentions it | Builds. Line 16 cites `docs/archive/SESSION_HANDOFF.md` §6 | Nobody | Public. R4 can fix line 16 cheaply: the cargo cache key changes once, the restore-keys still restore, no emulator is rebuilt, and with [skip ci] no build starts |
| .gitignore | Ignores build output and `.agents/` | Nothing | git uses it | Nobody | Public |
| HANDOFF.md | "Handoff for the next chat", 335 lines | owner-rules.md:47, 49, 100; core-updates.yml:12 (comment); scripts/check-core-updates.sh:17 (comment); STATUS.md:1140; docs/CBOY_NIGHT.md:8, 682, 686; reports/cleanup-2026-10-12/FINAL_REVIEW.md:73 and SUMMARY.md:62 | No | The orchestrator, at the end of the night | Notes (part 2). Its paste-this text (lines 3-7) names owner-rules.md, HANDOFF.md, STATUS.md, docs/CORE_UPDATES.md and TESTING.md. Line 283 says a push touching only those notes or docs/ (except docs/samples/) starts no build |
| STATUS.md | Build-by-build notes, 1,616 lines (153 KB) | owner-rules.md:47, 58 (include), 100, 159; README.md:60; HANDOFF.md (14 mentions); docs/CBOY_NIGHT.md (7), HARD_SYSTEMS, HISTORY, MANIC_PARITY, NATIVE_IOS_BLUEPRINT, PLATFORM_LIMITS, PRODUCT_SCOPE, SET_HW_RENDER_DESIGN, archive/, reports/ | No | The orchestrator | Notes (part 2). It links to TESTING.md (lines 132, 762, 811, 839), docs/PRODUCT_SCOPE.md (139), docs/REVIEW_COVERAGE.md (216), docs/HARD_SYSTEMS.md (392) and docs/SET_HW_RENDER_DESIGN.md (1288) |
| TESTING.md | The owner's phone test list, 965 lines | owner-rules.md:48, 59 (include), 165; README.md:60; STATUS.md (25); HANDOFF.md (12); docs/MANIC_PARITY.md:7 (link); CBOY_NIGHT, HARD_SYSTEMS, HISTORY, PRODUCT_SCOPE, archive/, reports/; native/ios/ContinuumApp.swift:683 (comment) | No | The orchestrator | Notes (part 2). Line 758 links to README.md |

## .github and .kiro

| File | What it is | Referenced from | Runs, builds or ships? | Writer tonight | Home |
| --- | --- | --- | --- | --- | --- |
| .github/workflows/ios.yml | The iOS build: Linux checks, then the .ipa on a Mac, then a Release | owner-rules.md:168; the comment in rust-toolchain.toml; HANDOFF; STATUS | Runs on a push to its listed paths, or when started by hand. It watches docs/samples/** (line 48) | Arcade job (a test step in `checks`, plus a push path for cores/continuum-arcade/**); cache job (a small edit to the cache comment) | Public. R5 changes only the notes text (lines 543-557) |
| .github/workflows/core-updates.yml | The weekly emulator update check: Mondays 07:00 UTC, which is 3:00 AM Eastern. Next run: 19 October | HANDOFF.md:158 | Runs. It writes and commits docs/CORE_UPDATES.md (lines 11, 56, 91, 117), and line 12 is a comment naming HANDOFF.md | Nobody | Public. Part 2 updates its paths |
| .kiro/steering/owner-rules.md | Kiro's rules, loaded in every session | HANDOFF.md:5 (paste text); crates/emulator-bridge/src/core_updates.rs:4; docs/archive/README.md:9 | Every Kiro session reads it. Its includes pull in docs/MANIC_PARITY.md (line 54), docs/PRODUCT_SCOPE.md (57), STATUS.md (58) and TESTING.md (59) | All six jobs read it; none of their checklists writes it | Tucked away. R4 changes line 62 (the archive line). Part 2 changes the four includes and the paths on lines 47-49, 66, 76, 100, 159-160 and 165 |

## docs/, top level

| File | What it is | Referenced from | Runs, builds or ships? | Writer tonight | Home |
| --- | --- | --- | --- | --- | --- |
| PRODUCT_SCOPE.md | Product rules for whoever works on the app: no web build, Android later, native look, the UI target, naming | owner-rules.md:57 (include), 76; README.md:61; STATUS.md:139; HARD_SYSTEMS.md:69; HISTORY.md:3; NATIVE_IOS_BLUEPRINT.md:20; archive/ | No | Arcade and Home screen jobs read it; nobody writes it | Notes (part 2). R4 edits it first: lines 67-69 keep the screenshots "as the design reference" (the shell is built now), lines 92-94 cite mobile-before/after, lines 170-171 cite the archive. Links MANIC_PARITY.md (124) |
| MANIC_PARITY.md | The feature checklist against Manic EMU | owner-rules.md:54 (include), 160; README.md:62; HANDOFF.md:186; ARCADE_CORE.md:233; PRODUCT_SCOPE.md:124 (link), 127; archive/; reports/. Code comments: crates/emulator-bridge/src/cores/options.rs:7, native/ios/ContinuumApp.swift:858, WebPlayerCore.swift:3, WebPlayers.swift:4, scripts/fetch-buildbot-cores.sh:11 | No | Arcade job (its arcade row) | Notes (part 2). Links ../TESTING.md (7) and HARD_SYSTEMS.md (28) |
| PLATFORM_LIMITS.md | What the owner's phone and install can do (signing, JIT) | owner-rules.md:66; HARD_SYSTEMS.md:4 (link), 29; NATIVE_IOS_BLUEPRINT.md:6; PRODUCT_SCOPE.md:7; archive/; native/ios/project.yml:721; scripts/build-core.sh:442 (inside parallel_n64's recipe) | No | Nobody | Notes (part 2). Links SET_HW_RENDER_DESIGN.md (100) |
| HARD_SYSTEMS.md | "Plain answers for the owner" on GameCube, Wii, Symbian and Dreamcast | STATUS.md:392 (link), 1275, 1282; MANIC_PARITY.md:28 (link), 68, 69, 147, 148; PLATFORM_LIMITS.md:143, 215; archive/ | No | Nobody | Notes (part 2). Links PLATFORM_LIMITS.md (4) |
| NATIVE_IOS_BLUEPRINT.md | The original iOS design, kept for its reasoning | crates/emulator-bridge/src/uniffi_api.rs:30; SET_HW_RENDER_DESIGN.md:19 (link); archive/ | No | Nobody | Notes (part 2). R4 fixes line 28 (the archive). Links SET_HW_RENDER_DESIGN.md (233, 394, 432, 506) |
| SET_HW_RENDER_DESIGN.md | The design for GPU-drawn emulators, 1,315 lines | STATUS.md:1288 (link); NATIVE_IOS_BLUEPRINT.md:19, 233, 394, 432, 506; PLATFORM_LIMITS.md:100; PRODUCT_SCOPE.md:150. Code comments: crates/emulator-bridge/src/gfx/gl_hw.rs:2, hw.rs:15, metal.rs:5, moltenvk.rs:1, vulkan_hw.rs:1; uniffi_api.rs:30; vulkan_probe.rs:3; native/ios/ContinuumApp.swift:333; native/switch-wrapper/continuum_switch_libretro.cpp:15, frame_gate.h:11, switch_engine.h:9, vulkan_stub_renderer.h:13; scripts/build-core.sh:329 (melonds recipe) and 432 (parallel_n64 recipe); scripts/fetch-moltenvk.sh:7 | No | Nobody | Notes (part 2). Links NATIVE_IOS_BLUEPRINT.md (19) |
| CORE_OPTIONS_STANDARD.md | How Continuum's own emulators declare their settings | STATUS.md:76; cores/continuum-gb/src/options.rs:2 | No | Nobody | Notes (part 2) |
| CORE_UPDATES.md | The weekly emulator update page | core-updates.yml:11, 56, 91, 117; scripts/check-core-updates.sh:3 and 37 (the output path); HANDOFF.md:6, 138 | Yes: the weekly workflow writes and commits it | The weekly run (next Monday) and any session's update check | Notes (part 2). Move it in ONE commit with check-core-updates.sh:37 and core-updates.yml:56, 117 |
| BUG_AUDIT.md | The 7 October bug audit, with where each item stands | STATUS.md:160, 161, 165, 180, 207; native/ios/DeltaSkinImport.swift:425 and LibraryShell.swift:110 (by name only: "BUG_AUDIT #25", "#33") | No | Nobody | Notes (part 2) |
| REVIEW_COVERAGE.md | Log of the line-by-line code review | STATUS.md:216 (link); reports/cleanup-2026-10-12/FINAL_REVIEW.md:73 | No | Nobody | Notes (part 2) |
| DELTA_SKIN_FORMAT.md | The skin format Continuum imports (Delta and Manic) | archive/README.md:11 | No | Nobody | Public, human pass (R6). Links samples/ (175-182) |
| HISTORY.md | "The story so far": a 10 October recap written to the owner, frozen at build 161 | Nothing. CloudSync.swift's "NO HISTORY" is unrelated | No | Nobody | **Delete (R4)**: STATUS.md and git history cover it |
| ARCADE_CORE.md | The cArcade plan | Nothing | No | Arcade job | Notes (part 2). Line 234 names README's arcade line |
| CBOY_EVENING.md | cBoy's evening notes | Nothing yet | No | cBoy job | Notes (part 2) |
| CBOY_EXACT_DRAWER.md | The working map for the exact drawer | HANDOFF.md:103, 107; STATUS.md:350, 357, 1434, 1536; CBOY_NIGHT.md:338, 713; cores/continuum-gb/src/fifo.rs:23, 41, 225, 240; cores/continuum-gb/tests/roms.rs:801, 808 | No | cBoy job | Notes (part 2) |
| CBOY_NIGHT.md | cBoy's notes from the night of 12-13 October | HANDOFF.md:97; STATUS.md:338, 1433, 1514; cores/continuum-gb/src/link.rs:23 | No | cBoy job | Notes (part 2) |
| CBOY_OPTIONS_INVENTORY.md | A generated table of cBoy's settings against other emulators' | STATUS.md:76, 148, 344; CBOY_NIGHT.md:692; CORE_OPTIONS_STANDARD.md:14; scripts/build-cboy-inventory.py:2 and 71 (which writes it) | Generated by scripts/build-cboy-inventory.py, run by hand. No workflow runs it | cBoy job | Notes (part 2), together with docs/option-sources/ and the script's paths |
| app-icon-source.png | The owner's icon artwork: 1024×1024 PNG, 1.6 MB (`file` checked) | native/ios/project.yml:600; STATUS.md:606; HANDOFF.md:171 (`make-app-icon.py --from`) | Input to make-app-icon.py, run by hand. The 13 sizes the app uses are committed | Nobody. The icons job is told never to change artwork | Public |
| app-icon.png | A preview of that icon at three sizes: 560×320 PNG, 214 KB (`file` checked, then viewed) | Nothing | No | Nobody | Public, and the best README picture (R2). R2 should not link into native/ios/Assets.xcassets, because the icons job is changing that folder tonight |
| library-mobile.png, mobile-player.png, settings-mobile.png, shot-detail.png, detail-artwork.png, mobile-before.png, mobile-after.png | Screenshots of the web app that was deleted (`file` checked: real PNGs, 59 KB to 1.2 MB) | PRODUCT_SCOPE.md:67-69, 92-93. Swift comments: LibraryShell.swift:4 and GameMetadata.swift:26 (library-mobile); GameArtwork.swift:777 (mobile-after); ContinuumApp.swift:5384, PlayerScreen.swift:9, TouchControls.swift:2544, 2721, 2807 (mobile-player) | No | Nobody | **Delete (R4)**. They show the old web app, so the README shouldn't use them |

## docs/ subfolders

| Path | What it is | Referenced from | Runs, builds or ships? | Writer tonight | Home |
| --- | --- | --- | --- | --- | --- |
| archive/README.md, archive/SESSION_HANDOFF.md (2,394 lines) | The September engineering notebook, plus its "do not follow" note | owner-rules.md:62; PRODUCT_SCOPE.md:170; NATIVE_IOS_BLUEPRINT.md:28; rust-toolchain.toml:16; native/ios/TouchControls.swift:1493, 3671. By name only: crates/emulator-bridge/src/achievements/rc_shim.c:7, cores/native_core.rs:200, 849, 2487, cores/options.rs:208, gfx/vulkan_hw.rs:238 | No | Nobody | **Delete (R4)** |
| ideas/voxel-pokemon.md | The G1R / voxel Pokémon idea, with its licence notes | STATUS.md:265 | No | Nobody (it's on the no-touch list) | Notes (part 2) |
| option-sources/ (24 files: *.keys, *.commit, mapping.tsv, standalone.tsv) | Raw settings lists from other emulators, which feed the cBoy inventory | scripts/build-cboy-inventory.py:2, 8, 38, 39 (reads); scripts/extract-core-option-keys.sh:4, 8 (writes); CBOY_OPTIONS_INVENTORY.md:4-5; CORE_OPTIONS_STANDARD.md:102-103; CBOY_EVENING.md:59, 63 | Only scripts run by hand. No workflow | Nobody. The arcade job may add itself to extract-core-option-keys.sh, so search again first | Notes (part 2), moved together with both scripts |
| reports/cleanup-2026-10-12/ (FINDINGS.md, SUMMARY.md, FINAL_REVIEW.md) | The 12 October cleanup review | STATUS.md:273-274; CBOY_NIGHT.md:695; reports/night-2026-10-13-batch-a.md:3 | No | Nobody (the folder is no-touch) | Notes (part 2). **FINDINGS.md:80 contains the retired support address**: take it out in the same commit. No other tracked file has it, binaries and the two skin zips included, and none of the 76 Releases' notes do either |
| reports/evening-2026-10-13.md | The report from the two-phone link batch | Nothing | No | Nobody now | Notes (part 2) |
| reports/night-2026-10-13-batch-a.md | The report from night batch A | HANDOFF.md:92; STATUS.md:272, 318, 1433, 1548 | No | Nobody | Notes (part 2) |
| reports/yellow-gb-177.txt | An activity log from a phone on build 177. It lists the owner's game file names | Nothing | No | Nobody | **Delete (part 2)**. The Yellow freeze it was for has been explained (the game file was cut off) |
| reports/evening-2026-10-13-icons.md, -text.md, -library.md (due tonight) | Reports from the icons, text and Home screen jobs | Not there yet | No | Those three jobs | Notes (part 2) |
| samples/ (5: two skins, their info.json files, sample_gba.png) | Sample skins | DELTA_SKIN_FORMAT.md:175-182 (links); ios.yml:48 (push path); scripts/check-delta-skin-sample.py:11-12, check-skins.sh:11, make-manic-skin-sample.py:2, 21, skin-check/main.swift:444 | Yes: the CI skin checks read them | Nobody | Public. It stays exactly where it is |
| side-projects/g1r-hud-off/ (5) | A personal patch that hides Apple's stats box in the owner's copy of G1R. It says it isn't part of Continuum | Nothing | No | Nobody | Notes (part 2). Not stale: it worked on the phone on 12 October and has redo steps for new G1R releases, so R4 keeps it. Nothing links to it, so it can move without fixing any references |

## What the later items need from this

**R2, the README**
- The README links to STATUS.md, TESTING.md, docs/PRODUCT_SCOPE.md and docs/MANIC_PARITY.md. All
  four are internal notes that move in part 2, so the new README is better without those links.
  Keep the LICENSE link.
- Picture: docs/app-icon.png.
- docs/ARCADE_CORE.md:234 says the arcade sentence goes out of date when cArcade ships, and leaves
  that to whoever writes the notes. Keep it to one plain sentence that's easy to find.
- STATUS.md still says build 182 is the newest, but Releases already has build-185-393f12c. Use the
  /releases/latest link and never a build number.

**R3, the About box.** `gh` works from this sandbox (reading is confirmed). Description, website
and topics are all empty. GitHub already shows the GPL-3.0 licence.

**R4, deleting the clutter**
- Delete the 7 screenshots, docs/archive/ (2 files) and docs/HISTORY.md.
- Fix in part 1: PRODUCT_SCOPE.md:67-69, 92-94 and 170-171; NATIVE_IOS_BLUEPRINT.md:28;
  rust-toolchain.toml:16; owner-rules.md:62. None of these is a Markdown link (they're all
  backticked paths), so the link check stays at 0 broken either way.
- List for part 2 (files part 1 may not edit). For the screenshots, the Swift comments at
  LibraryShell.swift:4, GameMetadata.swift:26, GameArtwork.swift:777, ContinuumApp.swift:5384,
  PlayerScreen.swift:9 and TouchControls.swift:2544, 2721, 2807. For the archive,
  TouchControls.swift:1493 and 3671 (by path), and by name only rc_shim.c:7,
  native_core.rs:200, 849, 2487, cores/options.rs:208 and gfx/vulkan_hw.rs:238.

**R5, the release notes.** The notes are the heredoc at ios.yml:543-555. Right after it, :556
appends the always-newest link and :557 appends "Built from <commit> on master", which puts a
commit hash in public text. The "Say how to install it" step (:647) names two signing apps, but
it's the run summary, not the notes, so it's outside R5. 39 of the 76 older Releases (builds 70 to
127) name a signing app in their notes. R5 only changes notes for new builds.

**R6, docs for visitors.** Only DELTA_SKIN_FORMAT.md, along with the docs/samples files it links
to. The internal bits in it: line 191 ("on the owner's phone (build 122, 5 October)") and line 193
("before build 109").

**R7, the part-2 plan**
- What editing each kind of referencing file costs, checked against the fingerprint code in
  scripts/build-core.sh and against ios.yml:
  - crates/, native/ and cores/continuum-gb/: no emulator rebuild, because cBoy and the app
    compile on every build anyway. [skip ci] stops the push from starting a build.
  - scripts/build-core.sh: line 329 is inside melonDS's recipe region, and lines 432 and 442 are
    inside parallel_n64's. Today a comment edit there rebuilds melonDS, or both N64 builds. The
    cache job is changing exactly this tonight ("a comment ... must stop rebuilding every
    emulator"), so check again after it lands.
  - scripts/fetch-buildbot-cores.sh and fetch-moltenvk.sh: not part of any emulator's
    fingerprint, but they are iOS push paths, so use [skip ci].
  - scripts/check-core-updates.sh, build-cboy-inventory.py, extract-core-option-keys.sh and
    core-updates.yml: editing them starts nothing.
  - rust-toolchain.toml and Cargo.lock: the cargo cache key changes and the restore-keys still
    restore. No emulator is rebuilt.
  - .kiro/**: not a push path.
- The Kiro includes are owner-rules.md:54, 57, 58 and 59.
- Three new reports land in docs/reports/ tonight. Also, the arcade, cBoy, text, Home screen and
  icons jobs are editing native/ios, crates and cores tonight, so search again for every line
  number above before part 2 edits anything.
- Take the retired address out of reports/cleanup-2026-10-12/FINDINGS.md:80.

## Tool

`.agents/tasks/check-links.py` (gitignored) lists every relative Markdown link and whether its
target exists. Today: 28 links, 0 broken. Run it from the repo root. Give it file names to check
only those files.
