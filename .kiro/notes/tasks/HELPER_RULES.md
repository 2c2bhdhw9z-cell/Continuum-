# Rules for every helper (14 October 2026)

You are one of several helper agents finishing the Continuum batch that stopped on 13 October at
6:25 PM Eastern, when the old account ran out of credits. Each helper has its own clone of the same
repository and pushes to the same `master`. The orchestrator (the main Kiro session) gave you one
job. The owner is a non-programmer who only uses an iPhone and tests only the finished .ipa.

## Setup

- Repo: `https://github.com/2c2bhdhw9z-cell/Continuum-.git` (git and gh are already
  authenticated). Branch: `master`.
- Your clone's path is in your prompt. If it doesn't exist:
  `git clone https://github.com/2c2bhdhw9z-cell/Continuum-.git <path>`. If it exists:
  `git -C <path> pull --rebase origin master`.
- Work only inside your clone, plus a scratch folder of your own,
  `/projects/sandbox/.scratch/<job>-tmp/`, for downloads, other people's source code and host
  builds. Never write into another helper's clone or into `/projects/sandbox/Continuum-` (the
  orchestrator's copy). Never commit downloaded third-party code, ROMs, BIOS files or test suites.
- **Other people's repositories:** this sandbox's global git config routes every
  `https://github.com/` URL through a gateway that only serves the owner's own repository, so
  cloning or fetching anything else (MAME, SameBoy, BlastEm, Mesen, binutils, an emulator's
  upstream) fails with "repository not found". For those, and for any script that clones them
  (`scripts/build-core.sh`, `scripts/check-core-updates.sh`), prefix the command with
  `GIT_CONFIG_GLOBAL=/dev/null`, for example
  `GIT_CONFIG_GLOBAL=/dev/null git clone --depth 1 https://github.com/mamedev/mame.git <dir>`.
  Never use that prefix for the Continuum repository itself: pushing needs the gateway.
  `curl` downloads (codeload.github.com, raw.githubusercontent.com, release assets) work as is.
- Read first, all of it: `<clone>/.kiro/steering/owner-rules.md` (the owner's rules; they apply to
  you), the "How to check work before pushing" section at the end of `<clone>/HANDOFF.md`, and your
  checklist `<clone>/.kiro/notes/tasks/<job>.json` (its hard_rules, facts and items).

## How the checklist applies now

- Paths in the checklist like `/projects/sandbox/.scratch/<x>-work` or `/projects/sandbox/Continuum-`
  mean YOUR clone.
- Files it names from the old sandbox are gone: anything under `.agents/`, `.scratch/icons-tools`,
  `.scratch/sameboy`, `.scratch/mealybug-src`, the old `per-core-fp` review. Work from what the
  checklist itself says, and re-fetch public sources into your scratch folder if you need them.
  Private scratch notes can go in `<clone>/.agents/` (gitignored), but anything the orchestrator or
  a later session needs must be committed (a report under `docs/` or the task file).
- There is NO deadline and no cutoff. Finish each item properly. Never weaken, skip, loosen or
  re-pin a test to make it pass; a pinned score only goes up.
- Its hard rules about which files you own still hold: other helpers are editing other parts of
  the same repository right now.

## Git

- Push straight to `master`. No branches, no pull requests, no force-push, never rewrite history.
- EVERY commit message ends with `[skip ci]`. Never start a build (no workflow dispatch, no
  re-run). The orchestrator starts the one build at the end.
- Commit messages are plain English a non-programmer can read, saying what changed for players.
- Stage files by name (never `git add -A` or `git add .`). Then
  `git pull --rebase origin master && git push origin master`; if it's rejected, pull --rebase and
  retry. On a conflict keep BOTH sides, then re-run your checks before pushing.
- Push each finished, green item at once. Never leave finished work only in the sandbox: the
  session can end at any moment.

## Progress, so the work survives if the session stops

- When an item is finished and pushed, edit its entry in `<clone>/.kiro/notes/tasks/<job>.json`:
  `"done": true` and a one-sentence plain-English `"result"` naming the commit hash. Commit and
  push that file too (`[skip ci]`). Only edit the entries for the items you were given. When every
  item in the file is done, set `"complete": true`.
- If an item genuinely cannot be finished (impossible here, never merely slow or hard), or you are
  running out of room to keep working: push everything finished and green, write exactly where you
  stopped and what is left into that item's entry (keep `"done": false`, add a `"progress"` field),
  push it, and say so in your report.

## Checks

- `export CARGO_BUILD_JOBS=2`, and `make -j2` / `ninja -j2` for anything else (this replaces the
  checklists' "3"): up to ten helpers share an 8-core, 30 GB machine, and running it out of memory
  kills everyone's work. If a build dies with signal 9 or "Killed", that was memory: wait a minute
  and retry with -j1 rather than giving up.
- `/root/.cargo` is read-only for helpers, so cargo can't download crates there. Use your own:
  `export CARGO_HOME=/projects/sandbox/.scratch/<job>-tmp/cargo-home` and
  `export RUSTUP_TOOLCHAIN=1.92.0-x86_64-unknown-linux-gnu`, in every shell that runs cargo.
- Run the checks your checklist names before every push.
- Swift: `swiftc -frontend -parse <file>` on every changed Swift file. There is no iOS SDK, Xcode or
  simulator here, so the parse check only catches writing mistakes. Be strict about iOS 16
  availability and real API signatures (copy patterns the code already uses): a Swift type error
  only shows up on the owner's build and costs them a build. A new Swift file must be listed in
  `native/ios/project.yml` `sources`.
- Never open or view an image whose bytes you have not checked with `file` first (a JPEG named
  .png once killed a whole chat session).
- Anything a player reads is short, casual and plain, like a person wrote it. No jargon.

## Don't touch

- `STATUS.md`, `TESTING.md`, `HANDOFF.md`: the orchestrator writes those from your report.
- Other jobs' task files in `.kiro/notes/tasks/`.
- `.kiro/steering/owner-rules.md` (unless your checklist explicitly allows one line).
- Anything under `.github/workflows/`, even where your checklist allows an edit. Kiro's GitHub
  rules never let a workflow change go straight onto master (only through a pull request), and
  the owner allows no pull requests, so workflow edits are parked. If your job needs one, write
  the exact change into your report and carry on without it.

## Your report (the last thing you return)

Plain and complete: each item you were given, done or not; the commit hashes; what the tests
prove; what is left and why; and everything the orchestrator must fold into STATUS.md, TESTING.md
and HANDOFF.md (phone checks in the owner's plain, casual words, one per thing a player can see,
and any TESTING.md lines that your change made out of date).
