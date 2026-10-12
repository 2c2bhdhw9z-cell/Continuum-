# Cleanup run summary (2026-10-12)

Repo `/projects/sandbox/Continuum-`, branch master. The run is the 38 commits `ee2c2f9..HEAD`
(`6193625` to `7284987`). Every one of them has `[skip ci]`. No iOS build was started.
Per-push evidence is in `VERIFICATION.md` (runs p1 to p8). The final numbers below were re-run on
HEAD `7284987` (logs: `/projects/sandbox/.cont-scratch/logs/final/`).

## Final numbers (HEAD 7284987)

| Check | Result |
|---|---|
| Workspace tests (`cargo test --workspace --features emulator-bridge/native-core`) | exit 0, **629 passed, 0 failed** |
| Clippy (same features) | **exit 0**, 0 warnings, 0 errors |
| cBoy tests (`cargo test --release`, after `fetch-test-roms.sh`) | exit 0, 78 passed, 0 failed |
| Mooneye DMG acceptance | **86/86** on Standard, 86/86 on Exact |
| dmg-acid2 / cgb-acid2 | **pass on both drawers** (Standard and Exact) |
| Mealybug (Exact drawer) | **5 passed / 50 failed** (the 5 pinned ones pass; the rest is the parked exact-drawer job) |
| Mooneye CGB | 0/5 (report only, not a gate) |
| Skins / players / Switch wrapper (last run, p8) | 720 checks 0 failed / 0 failed / 15/15 |

## Every commit in this run

| Commit | What it does |
|---|---|
| 6193625 | Git ignores the `.agents` workflow reports. |
| fd51c47 | Owner rules: new support email, QA Wolf gone, no made-up startup picture, pixel test pictures parked, the ★ rule. |
| 235a16a | Owner rules: the exact Game Boy drawer stays as a cBoy option, finished later. |
| 15fd146 | Owner rules: the exact drawer is the last job; stop and tell the owner before starting it. |
| cce544f | Clippy passes again and now runs on every build (K4). |
| 97c0367 | Feedback goes to ContinuumDev2c2b@gmail.com; the old address is gone from the files (IOS-03, K1). |
| fa79fa8 | QA Wolf removed completely (K6). |
| 4ecbd06 | Rust crates marked GPL-3.0-only, like LICENSE and README (K7). |
| 4cf71f3 | Editing cBoy no longer rebuilds every other core, and a cBoy edit starts a build (RUST-07, K5). |
| 6683d55 | Leftover made-up cBoy startup picture removed; the text says the game simply starts (K3). |
| bcc915e | cBoy's boot file, Advance BIOS and custom palette can be installed from the Continuum folder (CBOY-04). |
| f0302d7 | cBoy tests: Mealybug download pinned, skips cleanly without it, two warnings cleared (K8; also fixes RUST-17). |
| d317203 | cBoy "Picture drawing" option: Standard by default, Exact (experimental) as a choice (K2). |
| b5b0083 | cBoy: two stale code comments updated (K9). |
| 18bf18e | cBoy: phone motion reaches tilt and turn cartridges (CBOY-02). On its own it fails one test; 7b921c6 fixes it and both were pushed together. |
| 7b921c6 | Game Boy cartridge clock kept between sessions (`<game>.rtc`), catch-up fixed for MBC3 and HuC3 (CBOY-01, CBOY-03, CBOY-09). |
| 587228f | A damaged backup .zip shows an error instead of crashing the app (IOS-09). |
| d6b86d6 | Restore refuses a zip that isn't a Continuum backup and doesn't say "finished" after a failure (IOS-07). |
| db6918f | Back up says when some files couldn't go into the backup (IOS-08). |
| f14dc62 | Back up to Files copies one way, so deleted things never come back into the app (IOS-05). |
| c6272f0 | Feedback and crash-report forms are always open, whatever Settings folds (IOS-04). |
| 8ad4a7f | 3DS Amiibo: no tap by itself after a relaunch, and what a game saves goes back to the figure's file (RUST-03, RUST-04). |
| bdc3b72 | Symbian firmware is copied from the Continuum folder before a launch (RUST-01). |
| e124952 | Continuum Symbian's own reason is shown when a game won't start (RUST-02). |
| e61ff6f | NES plays on FCEUmm if a build has no Mesen (IOS-10). |
| 3103c8f | An auto-save that can't be resumed is moved to a free slot instead of being saved over (IOS-02). |
| 52863c3 | Backup prompt says what it really does, and asks again after a week of new saves (IOS-01). |
| 0feb2c0 | Restore keeps the newer copy of each file and puts the other one in Files, Continuum, Older copies (IOS-06). |
| 413a840 | TESTING: cleanup batch checks K1 to K5 added. |
| 8e6eca9 | Owner rules: take newer emulator versions after a thorough check each session; prefer cores without licence limits. |
| 2e6c1c1 | Plan: a one-time heads-up for players when an emulator was updated. |
| 0ab6fbd | Owner rules: open-licence swap plan for the five non-commercial cores. |
| a10ffab | Amiibo: the last tap is cleared before every game starts, even after a swipe-away (review fix R1 for RUST-03). |
| a6befb9 | Owner decision: swap out and remove the five non-commercial cores in the batch after the cleanup install. |
| 54e5bd6 | Owner decision: no MAME; arcade leaves the app until Continuum has its own arcade core. |
| 7683080 | Owner rules: real build time and core count, cBoy starts a build, drawer and licence swap in the new order, ★ rule word for word. |
| dc65b1b | cBoy options list: Game Boy Player rumble marked done; the Camera note says what is really there. |
| 7284987 | STATUS, HANDOFF, TESTING and the Manic list updated to match the cleanup, the licence swap and the new to-do order. |

## Found, not fixed

The fix phase only took CONFIRMED HIGH and CONFIRMED MEDIUM findings. Everything below was left
for one of two reasons: **SUSPECTED** (not proven; needs a phone run or a ROM scan first), or
**LOW** (real but small, saved for a later batch so this run stayed short).

SUSPECTED MEDIUM (needs proof first):
- RUST-05 Symbian keeps running and making sound while paused. Never run on a phone, and Symbian can't boot anywhere yet.
- RUST-06 Symbian reinstalls the game on every launch, on the main thread. Needs a phone run to see the freeze or lost progress.
- CBOY-05 A Game Boy ROM can be taken for an Advance game. No ROM set has been scanned to see if any real game hits it.

CONFIRMED LOW (left for a later batch):
- IOS-11 cBoy Super Game Boy picture about 3% too narrow under "Auto (Super Game Boy)". Small visual issue.
- IOS-12 Old Amiibo re-tapped by itself. The re-tap is fixed by 8ad4a7f and a10ffab. Still left: success is judged by matching the word "tapped".
- IOS-13 Restore never checks CRCs. Needs a damaged or short download to show up.
- IOS-14 Restore deletes the in-app Conflicts/ and Deleted/ folders. Those folders are hidden, so this is rare to notice.
- IOS-15 Tapping Back up twice drops the first export. Needs a double tap.
- IOS-16 The packed zip stays in tmp until the next backup. Only costs disk space.
- IOS-21 Tester text: a heading that doesn't exist, and long notes. Belongs to a text pass.
- IOS-22 Four shipped components missing from the credits list. Small licence-text fix.
- IOS-23 A restored icon choice marks the wrong icon. Small visual issue.
- IOS-24 Stale code comments. Comments only.
- RUST-08 A cBoy refusal can show an unrelated line. Message quality only, same root as CBOY-07.
- RUST-09 A graphics failure is called "the game file was refused". Message quality only.
- RUST-10 PSP frame markers flood the activity log. Diagnostics only.
- RUST-11 The clock-footer trim applies to every system. Only matters if an imported save is later exported to another emulator.
- RUST-14 Symbian never uses its JIT. Waits until Symbian boots at all (RUST-01 has to be confirmed first).
- RUST-15 The Amiibo stamp file is opened every frame. Small performance cost.
- RUST-16 CMake and ninja downloads aren't pinned. Build hygiene.
- RUST-17 The cBoy CI step's "SKIPPED" promise was false. **Fixed anyway by f0302d7** (K8).
- RUST-18 The VMU path comment cites the wrong source file. Comment only.
- RUST-19 Jargon in text testers can see (Azahar shader option, Amiibo hex id). Belongs to a text pass.
- CBOY-06 "Next start" options (sample rate, border) apply live. Only when changed mid-game.
- CBOY-07 Unsupported cartridge shows a code-like refusal. Message quality only.
- CBOY-08 Serial log grows forever. Only in a spin loop with no link partner.
- CBOY-09 Real-time catch-up stopped at 10 days. **Fixed with CBOY-01 in 7b921c6.**
- CBOY-10 "Light sensor" sunlight choice is always dark on iPhone. Affects Boktai only.
- CBOY-12 Jargon and debug switches in cBoy's Core settings. Belongs to a text pass.

Mixed LOW:
- IOS-17 Streaming unzippers refuse the backup zip. Proven only with Java; not known whether iOS Files cares.
- IOS-20 SMB shows raw errors and drops a typed port (confirmed); a blocked task pool (suspected). Small; SMB hasn't been tried on a phone.
- CBOY-11 Game Boy Camera comments claim a phone picture it never gets (confirmed); a possible 256-byte offset (suspected, Pan Docs not re-checked).

SUSPECTED LOW:
- IOS-18 The inflate loop has no autoreleasepool. Only matters with a multi-GB Files-compressed backup.
- IOS-19 Restore can reload skins and states while a game is running. Not seen happening.
- RUST-12 Symbian's files are never refreshed after an update. Only matters after Symbian works.
- RUST-13 Symbian shutdown order. A rare race, not seen happening.

Also left on purpose:
- The old support address is still in git history. Removing it means rewriting history and force-pushing, and that needs the owner's yes (review item R6).
- The exact Game Boy drawer's pixel work (Mealybug 5/55). Owner rule: it's the last job, and stop to ask before starting it.
- Non-blocking review items R2 to R5 from review iteration 2 were not changed.

## Needs a phone to confirm

Owner rule: "Done" means a phone showed it. Nothing in this run has been on a phone yet.

TESTING "This install only" (all four):
1. Settings → Send feedback → Send: addressed to ContinuumDev2c2b@gmail.com.
2. Game Boy and Game Boy Color set to cBoy, open a game without changing cBoy's settings: it plays normally.
3. In that game, Core settings → Picture drawing: Standard is picked, and Exact (experimental) can be picked.
4. Same page, note under "Start from a boot ROM file": says the game simply starts, no startup picture promised.

TESTING K (cleanup batch):
- K1 Old FCEUmm NES auto-save: the game starts fresh and says the old auto-save is in a slot that loads on FCEUmm (IOS-02).
- K2 Back up, save again, Restore: the newer save is kept and the older one is in Files, Continuum, Older copies (IOS-06).
- K3 Delete a game, Back up again: the game doesn't come back (IOS-05).
- K4 Feedback form on a fresh install: every part is open (IOS-04).
- K5 cBoy Picture drawing: Standard is picked, and Exact also plays a game (K2).
- K6 cBoy Pokémon Gold/Silver/Crystal: the in-game clock moves on while the game is closed (CBOY-01).
- K7 cBoy tilt game: it reacts to tilting the phone (CBOY-02).

Other fixes that only a phone can prove:
- Backup prompt wording, and it asking again after a week of new saves (IOS-01).
- A damaged zip or a non-Continuum zip on Restore shows a message, with no crash and no "finished" (IOS-09, IOS-07).
- "Backup saved, but N files couldn't go in" when some files fail (IOS-08).
- 3DS Amiibo: no tap by itself after a relaunch or swipe-away, and game writes saved to the figure's file (RUST-03, RUST-04).
- Symbian: firmware from `system/continuum-symbian/firmware/` is found, and a refusal shows Symbian's own words (RUST-01, RUST-02; TESTING S1).
- NES falls back to FCEUmm when Mesen isn't in the build (IOS-10). This needs a build without Mesen.
- cBoy boot file, Advance BIOS and custom palette work from the Continuum folder (CBOY-04).
