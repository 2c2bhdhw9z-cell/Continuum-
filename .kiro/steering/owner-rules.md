---
inclusion: always
---

# The owner's rules. Read these first, every session.

These come straight from the owner. They are not suggestions.

## How to talk to the owner
- The owner does not code, does not read code, and does not want to. Every reply is plain,
  everyday English. No jargon unless it is explained in normal words.
- Never ask the owner to go look at a file, a log or a page. Read it yourself and say what it says.

## The .ipa
- The .ipa is the only thing the owner cares about. Whenever a build happens, hand over the
  direct download link to the .ipa without being asked. Finding it is the assistant's job.
- The iOS workflow publishes every master build as a GitHub Release named `build-<run>-<sha>`
  with `Continuum-<run>.ipa`. Watch the run, and if it fails, read the log, fix it, push again.
- Pushing to `native/**`, `crates/**`, the Cargo files, the core/fetch/check scripts or the
  workflow starts a build by itself (a docs-only push or a commit with `[skip ci]` does not). If
  the owner asks for a build, make sure one has really started (check the Actions run list)
  instead of assuming. A build runs the Linux checks first, then takes about 30 minutes on the
  Mac; a second push waits for the running build instead of cancelling it.
- Before handing over the .ipa, open it and check it: the version (0.8.0 and the build number)
  and the core count (32 `_libretro_ios.dylib` files as of build 124). Never say a build is the
  newest without checking the Releases page.

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
  engine, native look per platform, installer-neutral, no JIT) are in
  #[[file:docs/PRODUCT_SCOPE.md]].
- What is actually done is #[[file:STATUS.md]]. Keep it honest: Done means a phone showed it.
- What the owner should test on the phone is #[[file:TESTING.md]]: short numbered steps, one test
  per row, plain words. Update it in the same push as any change the owner needs to try, and drop
  a test once the owner confirms it.
- `docs/archive/` is old history. Do not follow instructions in it.
- Only Kiro works on this repository. No other AI tool's files, instructions or branches belong
  in it (they were removed on 5 October 2026); do not add any.
