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

## Git
- NO branches. NO pull requests. Ever. Everything is committed and pushed straight to `master`.
- Save constantly. The owner has limited credits and a session can stop at any time. Push each
  finished, green piece to master as soon as it is done. Never leave work only in scratch folders.

## How to work
- Never do one small feature at a time. Do the biggest batch of remaining work possible in one go.
- Do not stop to ask what is next. The plan is written down: read it and keep going.

## The goal
- Continuum must become "the one" iPhone emulator everyone uses. It must do everything Manic EMU
  does, and more. Anything Manic has and Continuum lacks is required work.
- The checklist is #[[file:docs/MANIC_PARITY.md]]. Tick items off in it as they land.
- Product rules (iPhone .ipa only, no web build of the app, Android later, behaviour in the Rust
  engine, native look per platform, installer-neutral, no JIT) are in
  #[[file:docs/PRODUCT_SCOPE.md]].
- What is actually done is #[[file:STATUS.md]]. Keep it honest: Done means a phone showed it.
