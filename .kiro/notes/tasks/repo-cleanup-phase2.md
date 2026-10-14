# Repo cleanup, part 2: the plan

Written by part 1 of the cleanup (`repo-cleanup.json`, items R3 and R7) on 14 October 2026.
Part 2 starts once tonight's other jobs are finished and the one build has started.

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

The rest of this plan is item R7.
