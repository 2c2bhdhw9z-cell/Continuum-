# Repo cleanup, part 2: the plan

Written by part 1 of the cleanup (`repo-cleanup.json`, items R3, R5 and R7) on the night of 13
October 2026, against master `9e72cfa`. Part 2 starts once tonight's other jobs are finished and
the one build has started. Line numbers below are from that commit: tonight's jobs edit several of
these files, so search again before editing (every section gives the search).

Rules that still hold: every commit ends with `[skip ci]`; stage files by name; `git pull
--rebase origin master && git push origin master`, keeping both sides on a conflict; and a change
to a file in `.github/workflows/` never goes straight onto master (sections 2 and 6).

## 1. The GitHub "About" box (R3 could not set it from the sandbox)

GitHub refused both calls through the sandbox's GitHub connection: `PATCH repos/2c2bhdhw9z-cell/Continuum-`
and `PUT .../topics` both came back HTTP 403 ("Github returned a client error",
`X-Amzn-Errortype: UpstreamProviderClientError`), and `gh repo edit --description` got the same
403. Reading the repository works, and it reports admin rights for the account, so the limit is
the connection's token: it can push and make releases, but it can't change repository settings.
The GitHub power lists repo metadata edits as a known 403 with no workaround. Today the box is
empty: no description, no website, no topics. GitHub already shows the GPL-3.0 licence.

Set these three, exactly:

- Description: `An all-in-one emulator for iPhone. Free, open source, in public beta.`
- Website: `https://github.com/2c2bhdhw9z-cell/Continuum-/releases/latest`
- Topics (16): `emulator`, `emulation`, `ios`, `iphone`, `swift`, `swiftui`, `rust`, `libretro`,
  `retro-gaming`, `sideloading`, `game-boy`, `game-boy-advance`, `nes`, `snes`, `nintendo-ds`,
  `playstation`

From any session whose GitHub connection is allowed to edit settings:

```sh
gh api -X PATCH repos/2c2bhdhw9z-cell/Continuum- \
  -f description='An all-in-one emulator for iPhone. Free, open source, in public beta.' \
  -f homepage='https://github.com/2c2bhdhw9z-cell/Continuum-/releases/latest'
gh api -X PUT repos/2c2bhdhw9z-cell/Continuum-/topics \
  -f 'names[]=emulator' -f 'names[]=emulation' -f 'names[]=ios' -f 'names[]=iphone' \
  -f 'names[]=swift' -f 'names[]=swiftui' -f 'names[]=rust' -f 'names[]=libretro' \
  -f 'names[]=retro-gaming' -f 'names[]=sideloading' -f 'names[]=game-boy' \
  -f 'names[]=game-boy-advance' -f 'names[]=nes' -f 'names[]=snes' -f 'names[]=nintendo-ds' \
  -f 'names[]=playstation'
gh api repos/2c2bhdhw9z-cell/Continuum- --jq '[.description,.homepage,.topics]'
```

By hand it takes a minute in a browser signed in as the repository's owner, a phone included
(Safari's "Request Desktop Website" shows the gear): open the repository page, tap the gear next
to "About", paste the description and the website, type each topic, then "Save changes". Leave
"Releases" ticked so the sidebar keeps showing the latest build. Only the owner's own account can
do that, and the owner's rules say not to send them to pages, so try the calls first and only
offer the owner this as a one-minute favour if nothing else can.

## 2. The release notes (R5): written and checked, not landed

The new text for the "Publish the .ipa as a Release" step is ready, but part 1 did not push it.
Kiro's GitHub rules say a change to a workflow file never goes straight to the default branch,
whatever the reason, only through a pull request; the owner's rules say no pull requests, ever.
A helper can't settle that, so it's the orchestrator's call: open a pull request with just this
change and merge it, or take it up with the owner. No branch or pull request was made.

In `.github/workflows/ios.yml`, step "Publish the .ipa as a Release", replace everything between
`cat > /tmp/notes.md <<'EOF'` and the `EOF` line with this (keep the 10-space indent):

```text
          Continuum is an all-in-one emulator for iPhone, in public beta. Games and BIOS files aren't included, so you'll need your own.

          Download Continuum-${{ github.run_number }}.ipa below and install it with the signing app you normally use. Continuum.ipa is the same app under a name that never changes.

          Found a problem? In the app, tap Settings, then Send feedback. For one game, open it and tap ⋯, then Send feedback about this game. Or email ContinuumDev2c2b@gmail.com.

          The two get-task-allow files are only for turning on JIT, a speed boost some setups allow. Pick one if your signing app asks for an entitlements file; otherwise you can skip them.

          The newest build is always at this link:
```

Then delete the line right after `echo "$LATEST" >> /tmp/notes.md`, the one that starts
`printf '\nBuilt from %s on %s.\n'`: it puts the full commit hash in every release's notes. Touch
nothing else in the file (the arcade and cache jobs edit other parts of it).

Why it reads like that: each paragraph is one line because GitHub shows a line break in release
notes wherever the text has one, so the old 95-column wrapping broke sentences in half on a phone.
`${{ github.run_number }}` is filled in by Actions before the shell runs, quoted heredoc or not,
so the notes name the real file (`Continuum-186.ipa`); the same expression is already used a few
lines further down. Checked against master on 13 October: the YAML loads, `bash -n` passes on the
whole step with the expressions filled in, and the notes part runs and writes exactly the text
above plus the newest-build link. To check again after applying it:

```sh
python3 -c "import yaml,sys; yaml.safe_load(open(sys.argv[1]))" .github/workflows/ios.yml
python3 - <<'PY'
import re, subprocess, yaml
steps = [s for j in yaml.safe_load(open('.github/workflows/ios.yml'))['jobs'].values()
         for s in j.get('steps', []) if s.get('name') == 'Publish the .ipa as a Release']
v = {'github.run_number': '186', 'github.sha': '0' * 40,
     'github.repository': '2c2bhdhw9z-cell/Continuum-', 'github.ref_name': 'master'}
script = re.sub(r'\$\{\{(.*?)\}\}', lambda m: v[m.group(1).strip()], steps[0]['run'])
print('bash -n:', subprocess.run(['bash', '-n'], input=script, text=True).returncode == 0)
part = script[:script.index('echo "$LATEST" >> /tmp/notes.md')]
part += 'echo "$LATEST" >> /tmp/notes.md\ncat /tmp/notes.md\n'
print(subprocess.run(['bash', '-c', part], text=True, capture_output=True).stdout)
PY
```

The commit message must end with `[skip ci]`, or the push starts a build.

Not part of R5, for the orchestrator to decide: the next step, "Say how to install it", writes the
run's summary page and names two signing apps (ESign and Feather) plus the full commit hash.
That page is public too, but it isn't the notes.

## 3. Before starting

1. Every other job is finished: its checklist in `.kiro/notes/tasks/` says `"complete": true` or
   is deleted. They write `STATUS.md`, `TESTING.md`, `HANDOFF.md`, `docs/ARCADE_CORE.md`,
   `docs/CBOY_*.md` and `docs/reports/` tonight, and edit code that cites them.
2. `git pull --rebase origin master`, then `git ls-files docs` and the root listing. Anything new
   since this plan goes by the same rule: a working note moves (section 4), something for
   visitors stays.
3. Save every emulator's build fingerprint, to compare at the end (section 8):

   ```sh
   mkdir -p .agents
   for c in $(bash scripts/build-core.sh ios-pins | cut -f1); do
     printf '%s %s\n' "$c" "$(bash scripts/build-core.sh ios-fingerprint "$c" | tail -1)"
   done > .agents/fp-before.txt
   ```

## 4. The moves (commit 1, together with section 5)

One folder, `.kiro/notes/`, laid out like the old root and `docs/`: the three root notes go to
`.kiro/notes/`, and the internal docs to `.kiro/notes/docs/`. Because the layout is the same,
every link and every `docs/...` mention between the notes keeps working without an edit (a dry
run of exactly these moves, on a copy, broke 1 of 30 relative links; it's in 5.3).

```sh
mkdir -p .kiro/notes/docs
git mv HANDOFF.md STATUS.md TESTING.md .kiro/notes/
git mv docs/PRODUCT_SCOPE.md docs/MANIC_PARITY.md docs/PLATFORM_LIMITS.md docs/HARD_SYSTEMS.md \
       docs/NATIVE_IOS_BLUEPRINT.md docs/SET_HW_RENDER_DESIGN.md docs/CORE_OPTIONS_STANDARD.md \
       docs/BUG_AUDIT.md docs/REVIEW_COVERAGE.md docs/ARCADE_CORE.md docs/CBOY_EVENING.md \
       docs/CBOY_EXACT_DRAWER.md docs/CBOY_NIGHT.md docs/CBOY_OPTIONS_INVENTORY.md .kiro/notes/docs/
git rm docs/reports/yellow-gb-177.txt
git mv docs/reports docs/ideas docs/side-projects docs/option-sources .kiro/notes/docs/
```

`docs/reports/yellow-gb-177.txt` is deleted, not moved: an activity log from one phone that lists
the owner's game files, kept for a Yellow freeze that turned out to be a cut-off file. Nothing
names it.

Staying where they are, because they're for visitors or something uses them: `README.md`,
`LICENSE`, `docs/DELTA_SKIN_FORMAT.md` (the README links it), `docs/app-icon.png` (the README's
picture), `docs/app-icon-source.png` (`native/ios/project.yml` and `scripts/make-app-icon.py
--from` use it), `docs/samples/` (an `ios.yml` push path, and `scripts/check-skins.sh`,
`check-delta-skin-sample.py`, `make-manic-skin-sample.py` and `skin-check/main.swift` read it),
and `docs/CORE_UPDATES.md` unless section 6 moves it.

Add `.kiro/notes/README.md`:

```markdown
# Working notes

The project's working notes: what's done, what to test, the hand-off between sessions, design
records and reports. They moved here from the repository root and docs/ in October 2026, so the
front page stays tidy. Start with HANDOFF.md.

A path in these notes that starts with docs/ means this folder's docs/, except docs/samples/,
docs/DELTA_SKIN_FORMAT.md, docs/app-icon.png, docs/app-icon-source.png and docs/CORE_UPDATES.md,
which are still in the repository's own docs/. Paths to code (native/, crates/, cores/,
scripts/) are from the repository root.
```

(Drop `docs/CORE_UPDATES.md` from that list if section 6 moves it.)

## 5. References to update in the same commit

### 5.1 `.kiro/steering/owner-rules.md`

Search: `grep -n -E 'HANDOFF|STATUS\.md|TESTING|docs/|#\[\[file' .kiro/steering/owner-rules.md`

| Line | Now | Becomes |
| --- | --- | --- |
| 47-49 | `reading HANDOFF.md, then STATUS.md (its "Next up" section first) and TESTING.md section A` | `reading .kiro/notes/HANDOFF.md, then .kiro/notes/STATUS.md (its "Next up" section first) and .kiro/notes/TESTING.md section A` (re-wrap the lines) |
| 54 | `#[[file:docs/MANIC_PARITY.md]]` | `#[[file:.kiro/notes/docs/MANIC_PARITY.md]]` |
| 57 | `#[[file:docs/PRODUCT_SCOPE.md]]` | `#[[file:.kiro/notes/docs/PRODUCT_SCOPE.md]]` |
| 58 | `#[[file:STATUS.md]]` | `#[[file:.kiro/notes/STATUS.md]]` |
| 59 | `#[[file:TESTING.md]]` | `#[[file:.kiro/notes/TESTING.md]]` |
| 66 | `(docs/PLATFORM_LIMITS.md)` | `(.kiro/notes/docs/PLATFORM_LIMITS.md)` |
| 76 | `(docs/PRODUCT_SCOPE.md)` | `(.kiro/notes/docs/PRODUCT_SCOPE.md)` |
| 160 | `docs/MANIC_PARITY.md` | `.kiro/notes/docs/MANIC_PARITY.md` |

Lines 100, 159 and 165 name `STATUS.md`, `HANDOFF.md` and `TESTING.md` without a folder; the
names don't change, so they can stay. Line 62 (the removed archive) stays. Kiro's docs say
`#[[file:...]]` paths are from the workspace root and nothing excludes the `.kiro` folder; an
include that doesn't resolve shows an unresolved-reference marker in the session setup, so check
that in the first session after the push.

### 5.2 `.kiro/notes/HANDOFF.md`

The orchestrator rewrites it tonight, so search for the words, not the line:

- The paste-this text at the top says to read HANDOFF.md after the owner's rules. Give it the
  full path, `.kiro/notes/HANDOFF.md`: a new chat starts at the repository root.
- The build note (line 201 today) says a commit touching only STATUS.md, HANDOFF.md, TESTING.md
  or docs/ (but not docs/samples/) never needs `[skip ci]`. Make that "only `.kiro/notes/` or
  `docs/` (but not `docs/samples/`)". `.kiro/**` isn't an `ios.yml` push path, so it stays true.
- The mention of `docs/CORE_UPDATES.md` (line 56 today) stays right while the page stays in the
  repository's `docs/` (the notes README lists it as an exception); if section 6 moves it, it's
  right as well.

### 5.3 Links inside the notes

Only one relative link breaks: TESTING.md line 758 ("Before you start, read README.md") links to
`README.md`, and from `.kiro/notes/` the target must be `../../README.md`. Every other link
between the notes survives the mirrored layout (dry run: 30 relative links, 1 broken). No file
anywhere links to a repository file by its full GitHub URL, and the app only links to the
repository's front page (the credits list).

### 5.4 Scripts that read or write the notes

The cBoy settings inventory and its raw lists move together, so both scripts change in this
commit:

```sh
perl -pi -e 's#(?<!notes/)docs/(option-sources|CBOY_OPTIONS_INVENTORY\.md)#.kiro/notes/docs/$1#g' \
  scripts/build-cboy-inventory.py scripts/extract-core-option-keys.sh
python3 scripts/build-cboy-inventory.py
git diff -- .kiro/notes/docs/CBOY_OPTIONS_INVENTORY.md
```

That touches `build-cboy-inventory.py` lines 2, 8, 38, 39 and 71 and
`extract-core-option-keys.sh` lines 4 and 8 (the `(?<!notes/)` makes it safe to run twice).
Neither script is a push path or in any fingerprint, and no workflow runs them. On a copy of
`9e72cfa` the generator first reproduced today's inventory byte for byte, and after the move and
this edit the only lines that changed were 4 and 5, which quote the new folder. If the arcade job
added its own keys to `extract-core-option-keys.sh` tonight, check the new lines too.

### 5.5 The task folder

`.kiro/notes/tasks/README.md` and `HELPER_RULES.md` name the root notes and `docs/reports/`. If
every job is finished, the tasks README's own rule applies ("When a job finishes, delete its file
here"); delete the finished checklists, and update or delete those two. Don't edit a checklist
that is still running.

## 6. `docs/CORE_UPDATES.md`: only together with its script and its workflow

The weekly workflow writes this page: `scripts/check-core-updates.sh:37` (`OUT=...`) writes it,
and `.github/workflows/core-updates.yml:56` (`PAGE=...`) checks it for changes, commits it, and
`:117` prints it. If the page and the script moved without the workflow, the next run (Monday 19
October, 3:00 AM Eastern) would write the new page, find nothing changed at the old path and
quietly commit nothing, every week.

- Option A, recommended: leave `docs/CORE_UPDATES.md` where it is. It's a generated page in
  plain English, it's in `docs/` rather than on the front page, and nothing has to change.
- Option B: move it in one pull request that carries `git mv docs/CORE_UPDATES.md
  .kiro/notes/docs/`, `check-core-updates.sh` lines 3 and 37, and `core-updates.yml` lines 11,
  12, 56, 91 and 117. It can share a pull request with the release notes in section 2, since both
  are workflow changes and need the same decision. Check after: the YAML loads, `bash -n` passes
  on each `run:` block, and `GIT_CONFIG_GLOBAL=/dev/null bash scripts/check-core-updates.sh`
  writes `.kiro/notes/docs/CORE_UPDATES.md` (then `git checkout` the page if only its timestamp
  moved).

## 7. Code comments (commit 2)

Comments in code that cite a moved note. One command handles the paths (it skips the notes
themselves, the workflows and the vendored code, and is safe to run twice):

```sh
N='PRODUCT_SCOPE|MANIC_PARITY|PLATFORM_LIMITS|HARD_SYSTEMS|NATIVE_IOS_BLUEPRINT|SET_HW_RENDER_DESIGN|CORE_OPTIONS_STANDARD|BUG_AUDIT|REVIEW_COVERAGE|ARCADE_CORE|CBOY_[A-Z_]+'
git grep -l -I -E "docs/($N)\.md" -- . ':!.kiro/notes' ':!.github' ':!crates/emulator-bridge/vendor' \
  | xargs -r perl -pi -e "s#(?<!notes/)docs/(($N)\.md)#.kiro/notes/docs/\$1#g"
```

What it touches today (search again first):

- Rust, cBoy and cArcade: `cores/continuum-arcade/Cargo.toml:2`, `cores/continuum-arcade/src/lib.rs:2`,
  `cores/continuum-gb/src/fifo.rs:23, 41, 225, 240`, `link.rs:23`, `options.rs:2`,
  `cores/continuum-gb/tests/roms.rs:801, 808`, `crates/emulator-bridge/src/cores/options.rs:7`,
  `gfx/gl_hw.rs:2`, `gfx/hw.rs:15`, `gfx/metal.rs:5`, `gfx/moltenvk.rs:1`, `gfx/vulkan_hw.rs:1`,
  `uniffi_api.rs:30`, `vulkan_probe.rs:3`.
- The app: `native/ios/ContinuumApp.swift:333, 858`, `WebPlayerCore.swift:3`, `WebPlayers.swift:4`,
  `project.yml:721`, and `native/switch-wrapper/continuum_switch_libretro.cpp:15`,
  `frame_gate.h:11`, `switch_engine.h:9`, `vulkan_stub_renderer.h:13`.
- Scripts: `scripts/build-core.sh:329, 432, 442`, `fetch-buildbot-cores.sh:11`,
  `fetch-moltenvk.sh:7`.
- The steering file's lines 54, 57, 66, 76 and 160 too, if 5.1 wasn't done by hand first.

By hand: `native/ios/ContinuumApp.swift:683` "(see TESTING.md)" and `scripts/check-core-updates.sh:17`
"See HANDOFF.md." become `.kiro/notes/TESTING.md` and `.kiro/notes/HANDOFF.md`.
`core-updates.yml:12` says "HANDOFF.md" too, but it's a workflow, so it waits for section 6, and
the bare name is still right. Mentions by name only stay as they are: "BUG_AUDIT #25"
(`DeltaSkinImport.swift:425`), "BUG_AUDIT #33" (`LibraryShell.swift:117`), "TESTING S1"
(`ContinuumApp.swift:3425`).

Also left from part 1: 16 comments name files part 1 deleted (`e3c185b`). Say the file was
removed and is in git history, for example "the old web app screenshot mobile-player.png (removed;
`git show e3c185b^:docs/mobile-player.png`)":

- The web app screenshots: `native/ios/ContinuumApp.swift:5392`, `GameArtwork.swift:777`,
  `GameMetadata.swift:26`, `LibraryShell.swift:4`, `PlayerScreen.swift:9`,
  `TouchControls.swift:2544, 2721, 2807`.
- The September notebook (`docs/archive/SESSION_HANDOFF.md`): by path in
  `native/ios/TouchControls.swift:1493, 3671`; as "SESSION_HANDOFF" in
  `crates/emulator-bridge/src/achievements/rc_shim.c:7`, `cores/native_core.rs:200, 849, 2487`,
  `cores/options.rs:208` and `gfx/vulkan_hw.rs:238`.

What these edits cost: comments in `crates/`, `native/`, `cores/continuum-gb/` and
`cores/continuum-arcade/` rebuild no emulator (cBoy, the engine and the app compile on every
build anyway), and `[skip ci]` keeps the push from starting one. `fetch-buildbot-cores.sh` and
`fetch-moltenvk.sh` are push paths but in no fingerprint. The three `build-core.sh` lines sit
inside melonDS's (329) and ParaLLEl N64's (432, 442) recipe regions: before tonight's cache work
a comment there rebuilt that emulator, and the cache job's K1 makes full-line comments stop
counting. Measured on `9e72cfa`, before K1: editing those three comments moves the fingerprints
of melonDS, ParaLLEl N64 and ParaLLEl N64's JIT build, and nothing else. The fingerprint check in
section 8 settles it; if a fingerprint moved, put those three lines back and leave them for later.

Sections 4, 5 and 7 were rehearsed on a throwaway local copy of `9e72cfa`: the perl commands
changed only comments and the two script paths, the inventory came out with only its two path
lines different, every steering include resolved once 5.1 was done, the link check found 0 of 30
broken after the 5.3 fix, `scripts/check-skins.sh` passed (720 checks, and the Delta sample), and
`scripts/check-core-cache.sh` passed (105).

## 8. Checks that prove nothing broke

Run them all before the last push.

```sh
# 1. Moves are renames, not copies plus edits gone wrong.
git diff --cached -M --stat | tail -3

# 2. No root-relative path to a moved note is left outside the notes (prints nothing;
#    with option A, docs/CORE_UPDATES.md isn't in the list on purpose).
N='PRODUCT_SCOPE|MANIC_PARITY|PLATFORM_LIMITS|HARD_SYSTEMS|NATIVE_IOS_BLUEPRINT|SET_HW_RENDER_DESIGN|CORE_OPTIONS_STANDARD|BUG_AUDIT|REVIEW_COVERAGE|ARCADE_CORE|CBOY_[A-Z_]+'
git grep -n -I -P "(?<!notes/)docs/(($N)\.md|option-sources|reports|ideas|side-projects)" \
  -- . ':!.kiro/notes' ':!crates/emulator-bridge/vendor'

# 3. Every steering include exists.
grep -o '#\[\[file:[^]]*\]\]' .kiro/steering/owner-rules.md | sed 's/^#\[\[file://; s/\]\]$//' \
  | while read -r f; do [ -f "$f" ] && echo "ok $f" || echo "MISSING $f"; done

# 4. No emulator rebuilds (prints the line only if one would).
for c in $(bash scripts/build-core.sh ios-pins | cut -f1); do
  printf '%s %s\n' "$c" "$(bash scripts/build-core.sh ios-fingerprint "$c" | tail -1)"
done > .agents/fp-after.txt
diff .agents/fp-before.txt .agents/fp-after.txt && echo "no emulator rebuilds"
bash scripts/check-core-cache.sh

# 5. The skin checks still find docs/samples, and the edited scripts parse.
bash scripts/check-skins.sh
for s in scripts/extract-core-option-keys.sh scripts/check-core-updates.sh; do bash -n "$s"; done

# 6. Only for the commit that edits code comments, before committing it.
export CARGO_BUILD_JOBS=2
cargo test --workspace --features emulator-bridge/native-core
cargo clippy --workspace --features emulator-bridge/native-core
(cd cores/continuum-gb && scripts/fetch-test-roms.sh && cargo test --release)
for f in $(git diff --name-only HEAD -- '*.swift'); do swiftc -frontend -parse "$f" || echo "FAILED $f"; done

# 7. No mail address but the support one (prints nothing).
git grep -n -I -i -E '[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}' -- . ':!crates/emulator-bridge/vendor' \
  | grep -v -i 'ContinuumDev2c2b@gmail.com' | grep -v -i 'noreply'
```

And the links, with this checker (part 1's, from the repo root; 0 broken is the pass):

```sh
python3 - <<'PY'
import os, re, subprocess
link = re.compile(r'!?\[(?:[^\[\]]|\[[^\]]*\])*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)')
bad = total = 0
for md in subprocess.check_output(['git', 'ls-files', '*.md'], text=True).split():
    fenced = False
    for no, line in enumerate(open(md, encoding='utf-8'), 1):
        if re.match(r'\s*(```|~~~)', line):
            fenced = not fenced
            continue
        if fenced:
            continue
        for t in link.findall(re.sub(r'`[^`]*`', '', line)):
            if re.match(r'[a-zA-Z][a-zA-Z0-9+.-]*:', t) or t.startswith('#'):
                continue
            total += 1
            path = os.path.normpath(os.path.join(os.path.dirname(md), t.split('#')[0]))
            if not os.path.exists(path):
                bad += 1
                print(f'BROKEN {md}:{no} -> {t}')
print(f'{total} relative links, {bad} broken')
PY
```

Then look at the result the way a visitor will: `git ls-files | grep -v /` should list only
`.gitignore`, `Cargo.lock`, `Cargo.toml`, `LICENSE`, `README.md` and `rust-toolchain.toml`, and
`git ls-files docs` only `DELTA_SKIN_FORMAT.md`, the two app icon pictures, `samples/` and
(option A) `CORE_UPDATES.md`.

## 9. Public lines to revisit once tonight's work lands

The README was written for what is true on 13 October. Each line below changes only when the
work is in a build, and moves into the "Played on an iPhone" table only once a phone has shown
it (STATUS.md's rule: Done means a phone showed it).

- Arcade (README line 68, "Arcade games aren't playable right now. Continuum is getting an
  arcade emulator of its own..."): when cArcade is in a build, add an Arcade row to "In the app,
  not much tested yet" (Arcade, cArcade), say in that bullet which games or boards it plays and
  that they're zip sets, and make the cBoy section (line 90) about Continuum's own emulators,
  cBoy and cArcade. The About box can take an `arcade` topic then.
- SNES (line 26, `| SNES | bsnes |`): if Mesen 2 ships as a second SNES choice, make it
  "bsnes, or Mesen". Not before.
- Game Boy (line 27) and the cBoy section (lines 92-95): when they're in a build, the
  two-phone link cable for Game Boy games and cBoy's "Colour for black-and-white games" (off by
  default) can each get a short "(new)" line under "What it does". "Advance games on cBoy are
  still early" stays until a phone has played one.
- Systems moving up a table: PSP (line 43) once the picture fixes are confirmed on a phone, and
  any other "not much tested" system the owner or a tester confirms. Note that STATUS.md's
  Systems table still calls SNES, Mega Drive and Game Gear "Built, untested", while TESTING.md
  records all three passing on build 181; the README follows TESTING.md, so bring STATUS.md into
  line.
- The 3DS stutter (line 66): drop "there can be a short stutter after a scene change" only when a
  phone shows it gone.
- JIT (lines 71-73): drop "no tester has tried it yet" once one has.
- Online play (line 88): drop "(new, so it hasn't had much testing)" once two phones have played.
- Symbian / N-Gage is left out of the README on purpose (no phone has run a game); add it to "not
  much tested" once one has.
- Light mode, if it ships: "Dark and light looks" under "What it does" (the app says Dark, Light,
  System).
- Labels the README and the skin guide quote, which the shorter-text and Home screen jobs may
  rename: the + button and press-and-hold on it, Wi-Fi transfer, Settings, BIOS, "Install a BIOS
  from the Continuum folder", Settings, About, "Open-source credits", Settings, Technical details,
  Settings, "Send feedback", ⋯ "Send feedback about this game"; and in `docs/DELTA_SKIN_FORMAT.md`
  "Open the skin library", "Import skins", "Import .deltaskin", "Move the on-screen controls",
  "Edit this skin", "Import for", "Button sounds" and the ⋯ menu's "Skin...". A quick check:
  `grep -rn --include=*.swift -F '"<label>' native/ios`.
- `docs/PRODUCT_SCOPE.md` still describes "a thin status strip above the tab bar" and other
  details of the Home screen; it's a working note, so that's for whoever updates the notes after
  the Home screen job, not the README.

## 10. After part 2

- Delete `repo-cleanup.json`, `repo-cleanup-inventory.md` and this plan from `.kiro/notes/tasks/`
  in a docs-only push, once every item here is done (sections 1, 2 and 6 may stay open longer,
  waiting on GitHub settings and the workflow decision; keep this file until they're settled).
- Tell the owner in one plain line: the GitHub page now shows the README, the licence and the
  code, and the working notes are tucked away in `.kiro/notes/`.
