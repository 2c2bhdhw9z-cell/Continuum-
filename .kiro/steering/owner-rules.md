---
inclusion: always
---

# The owner's rules. Read these first, every session.

These come straight from the owner. They are not suggestions.

## How to talk to the owner
- The owner does not code, does not read code, and does not want to. Every reply is plain,
  everyday English. No jargon unless it is explained in normal words.
- Never ask the owner to go look at a file, a log or a page. Read it yourself and say what it says.
- The owner is on US Eastern time (UTC-4 in summer, UTC-5 in winter). Give every time in Eastern.
  Never assume Pacific.
- Never ask the owner to re-test or re-answer anything they already covered, in a screenshot or an
  earlier message. Work it out from what they sent. Never put a test back on their list.
- When handing over a build, write the full test steps out in that same reply. Never say "the
  tests from my last message"; the owner will not scroll back for them.

## The .ipa
- The .ipa is the only thing the owner cares about. Whenever a build happens, hand over the
  direct download link to the .ipa without being asked. Finding it is the assistant's job.
- The iOS workflow publishes every master build as a GitHub Release named `build-<run>-<sha>`
  with `Continuum-<run>.ipa`. Watch the run, and if it fails, read the log, fix it, push again.
- Pushing to `native/**`, `crates/**`, `cores/continuum-gb/**` (cBoy), the Cargo files, the
  core/fetch/check scripts or the workflow starts a build by itself (a docs-only push or a commit
  with `[skip ci]` does not). If the owner asks for a build, make sure one has really started
  (check the Actions run list) instead of assuming. A build runs the Linux checks first, then
  takes about 6-8 minutes on the Mac when the cores are cached, or about 45-50 minutes when a
  core, a patch or `scripts/build-core.sh` changed (a cBoy-only change stays cheap); a second
  push waits for the running build instead of cancelling it.
- Before handing over the .ipa, open it and check it: the version (0.8.0 and the build number)
  and the core count (36 `_libretro_ios.dylib` files from the licence-swap install on: 33 cores plus 3 JIT builds; build 180 had 38). Never say a build is the
  newest without checking the Releases page.
- With every .ipa, give a short list of easy things to try on it. The owner often tests at work.

## Git
- NO branches. NO pull requests. Ever. Everything is committed and pushed straight to `master`.
- Save constantly. The owner has limited credits and a session can stop at any time. Push each
  finished, green piece to master as soon as it is done. Never leave work only in scratch folders.

## How to work
- Never do one small feature at a time. Do the biggest batch of remaining work possible in one go.
- Do not stop to ask what is next. The plan is written down: read it and keep going.
- Start every session by reading HANDOFF.md, then STATUS.md (its "Next up" section first) and
  TESTING.md section A, then carry on from there. Before a session ends (or when the chat gets
  long), update HANDOFF.md and "Next up" so the next chat can pick up with no explaining.

## The goal
- Continuum must become "the one" iPhone emulator everyone uses. It must do everything Manic EMU
  does, and more. Anything Manic has and Continuum lacks is required work.
- The checklist is #[[file:docs/MANIC_PARITY.md]]. Tick items off in it as they land.
- Product rules (iPhone .ipa only, no web build of the app, Android later, behaviour in the Rust
  engine, native look per platform, installer-neutral, JIT when available but never required) are in
  #[[file:docs/PRODUCT_SCOPE.md]].
- What is actually done is #[[file:STATUS.md]]. Keep it honest: Done means a phone showed it.
- What the owner should test on the phone is #[[file:TESTING.md]]: short numbered steps, one test
  per row, plain words. Update it in the same push as any change the owner needs to try, and drop
  a test once the owner confirms it.
- `docs/archive/` is old history. Do not follow instructions in it.

## Already decided by the owner (do not reopen)
- No computer, ever, not even once or borrowed. Never suggest anything that needs one (a pairing
  file, a "JIT certificate"). So the OWNER cannot use JIT (docs/PLATFORM_LIMITS.md).
- JIT IS STILL REQUIRED FOR EVERYONE ELSE (owner, 7 October 2026, very angry it was missing).
  "No JIT" only ever meant: everything must keep working without JIT. Any user who can enable JIT
  (development-signed and a JIT enabler attached) must get the JIT speed automatically, and every
  core that has a JIT should use it when available. Never answer "your rules say no JIT" again.
- Before starting the Switch (road steps 10 to 12), raise JIT with the owner first: iPhone Switch
  emulators need it.
- The Beetle PSX boot on build 98 was enough to move on to the 3DS. Do not reopen it.
- The skin layout editor is not Done until the owner says the skin is right.
- iOS controls keep Apple's glass look; Android will use Material's own look. Same behaviour,
  different appearance, on purpose (docs/PRODUCT_SCOPE.md).
- Only Kiro works on this repository. No other AI tool's files, instructions or branches belong
  in it (they were removed on 5 October 2026); do not add any.
- Public beta from 6 October 2026: strangers download and test the app. Keep what a tester sees
  plain: technical read-outs only behind the (i) button and Settings, Technical details. The owner
  posts the .ipa itself in Telegram and Reddit, so testers only see the app: every line they read
  must sound like a person wrote it, casual and short, never like documentation.
- **Support and feedback email: ContinuumDev2c2b@gmail.com** (owner, 12 October 2026;
  `FeedbackDestination.email` in `native/ios/Feedback.swift`). The old address is retired: it must
  not appear anywhere in the repository, not in the app, the README or any note, history included.
- Outside testing services are done with: Momentic (simulator only, needs a full rebuild: the
  owner said no), Apptest.ai (rejects an .ipa without an Apple certificate). QA Wolf was removed
  completely on 12 October 2026 (no workflow, no MCP server, no setup steps, no mentions in the
  notes); never bring it back. Do not offer a built-in self-test either; the owner has proved most
  systems.
- **No made-up startup picture, ever** (owner, 11 and 12 October 2026). cBoy, and any core this
  project writes, starts the game straight away, like mGBA, SameBoy and Gearboy do. A boot
  picture only plays when the user's own real boot file is there. No logo, no name flash, no
  "our own boot animation". Remove any code, option text or note that says otherwise.
- **cBoy's pixel-by-pixel drawer stays, as a cBoy core option** (owner, 12 October 2026: every
  extra setting cBoy offers is wanted, because Continuum is meant to be the go-to emulator). The
  default is the proven per-line drawer; the pixel-by-pixel one is the "Exact (experimental)"
  choice. The owner would love it finished one day. **It comes right after the cBoy jobs on the
  to-do list** (owner, 12 October 2026; it is no longer the last item), and "Next up" in
  STATUS.md and HANDOFF.md list it there. **Before starting it, STOP and tell the owner** so
  they can turn Kiro's thinking up to max first. Do not begin any of it, not even a first look, until they
  reply that it is set.
  Earlier sessions burned through the owner's usage guessing its test pictures (Mealybug) one
  pixel at a time. When it is picked up again, never guess pixel by pixel: follow how a proven
  accurate emulator (SameBoy) draws, and compare against it. Do not make it the default until
  the owner has seen it work on the phone.
- **Emulator updates (owner ⭐, 12 October 2026): always use the newest version of every core,
  as long as it does not break anything major compared with the version we ship.** Not
  automatic or a bot: Kiro checks at the start of every session, for every core (built from
  source and downloaded), and takes each newer one only after a thorough check. Read what
  changed upstream, confirm our patches still apply, build it, run every check, and confirm
  in-game saves and old save states still load. If an update would break, remove or slow down
  something players already have, do not take it, and tell the owner why. Take the good ones in
  the next batch (one build), and tell the owner plainly which consoles got newer emulators.
  This replaces the old "let the owner decide each update" rule.
  **Players get a heads-up (owner, 12 October 2026).** The first time someone opens a game on
  an emulator that changed since their last install, the app shows one short, casual line, once
  per emulator version, and never on a first install. Example: "Heads up: the SNES emulator
  (Snes9x) just got an update from the people who make it. If anything seems off, send us
  feedback." If that update means older save states can't load, the line says so and adds that
  in-game saves are fine. Build this with the first batch of core updates.
- **Licences (owner, 12 October 2026):** the owner wants Continuum free of outside licence
  limits in the long run. That is why they want cores of their own (like cBoy). Five cores
  forbid any commercial use: Snes9x, Genesis Plus GX, PicoDrive, FinalBurn Neo and MAME
  2003-Plus. When a system gets a core of our own, or a well-made core with an open licence,
  prefer that one.
  **Owner's decision (12 October 2026): get every licence limit out of the app.** All five
  non-commercial cores are swapped out AND removed completely in ONE batch, folded into the same
  install as the cleanup: from the app, the build scripts, the checksums, the routing, the
  settings, the credits page and the docs. Before that batch
  ships, check every replacement thoroughly off the phone: build host versions, boot real test
  content through the engine, check speed, in-game saves and save states, and that each file
  type still routes. Then tell the owner which systems changed. The licences that remain
  (GPL, MPL, zlib, MIT, Apache, LGPL) put no limits on what the app does, what it costs or how
  it runs. They only require the credits page and public source code. They can only go away
  when a system has a core of our own. Never delete a remaining core's credit or licence
  notice: that is what allows the app to ship it at all.
  **Swap plan (checked against the libretro iOS buildbot on 12 October 2026):**
  - SNES: bsnes (GPLv3) as the default. Mesen 2 (GPLv3, already in the app for NES) also plays
    SNES and becomes a second choice. Both need more CPU than Snes9x, so check the speed.
  - Master System, Game Gear, SG-1000: Gearsystem (GPLv3). Mesen 2 also plays these.
  - Genesis / Mega Drive, Sega CD, 32X: BlastEm (GPLv3; its libretro info now lists MD, CD and
    32X). Unknowns: speed on iPhone without its PC speed-up, and how mature its CD and 32X
    support is. ClownMDEmu (AGPLv3, MD and CD) is the backup; it is not on the iOS buildbot, so
    it would be built from source.
  - **Arcade: NO MAME, no stand-in (owner, 12 October 2026).** FinalBurn Neo and MAME 2003-Plus
    are removed with the rest, and nothing replaces them for now. Arcade leaves the app until
    Continuum has an arcade core of its own. That core is required future work: keep it on the
    to-do list (after the exact Game Boy drawer and the phone test rounds; see "Next up" in
    STATUS.md), and treat Arcade in
    docs/MANIC_PARITY.md as "removed by the owner's choice, own core planned", not as a gap
    to fill with someone else's core. While arcade is out, an arcade game a player already
    imported stays in the library and is never deleted. Opening it shows one plain, casual
    line, for example: "Arcade games aren't playable right now. We're building our own arcade
    emulator." Don't ask "Which system?" for it, and never crash. Remove the arcade test
    rows, and the `neogeo.zip` BIOS mention, from TESTING.md.
- When the owner puts a ★ on something, save it permanently: in this file if it is about Continuum, or as a Kiro learning if it applies everywhere.
  A ⭐ counts the same. Kiro may also save general learnings on its own whenever something is
  worth remembering.
