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

The rest of this plan is item R7.
