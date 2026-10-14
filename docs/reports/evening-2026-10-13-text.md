# Shorter text everywhere in the app (13 and 14 October 2026)

At 6 PM on 13 October the owner sent 13 screenshots of Settings with nearly every paragraph circled
and said: "I want stuff that can be condensed. Condensed and made shorter." This is the running list
of what changed. Everything is **Built, untested**: nothing here has been on a phone yet.

Only words changed. No switch, button or choice does anything different, keeps a different setting
or starts with a different default. The technical read-outs (the small grey capitals with
typewriter-style text under them) were moved into **Settings → TECHNICAL DETAILS**, word for word,
each under the name of the section it came from.

- T1, the first half of Settings: `ec63823` (an earlier session).
- T2, the second half of Settings: this list, below.
- T3, the other screens: below, once it is done.

## T2: the second half of Settings

### COVER ART

- The switch: "On, so the library shows real box art." / "Off, so every game shows its generated
  plate." became **"On. Your games get real box art."** / **"Off. Games show a title card
  instead."**
- Three paragraphs (the libretro disclosure, how the cover lists work and what each costs in
  megabytes, covers borrowed from another system) became one: **"Covers come from libretro's free
  cover server, looked up by each game's file name. Nothing else is sent."**
- The paragraph under the three buttons became **"Both clear buttons just free up space. Anything
  still needed downloads again."**
- Gone from the section: the megabyte sizes of the cover lists, the "box art from Game Boy"
  explanation, and how long a missing cover is remembered. A game whose title matches several
  covers still lets you pick one on its card.
- Read-outs: **Stored**, **Cover lists** and **Last** moved to TECHNICAL DETAILS → COVER ART. The
  section keeps one plain line: "78 covers saved (27.0 MB)", or "No covers saved yet".

### LAYOUT

- Removed: "These two are remembered between launches, and both are about the library. The
  on-screen game controls are a separate setting with a confusingly similar name: they are in
  ON-SCREEN CONTROLS above." Both rows already say what they do.

### PSP

- The whole section is gone (the owner wrote "why are we here" on it). It held one note: "PSP games
  don't need a BIOS. .cso, .iso, .chd and EBOOT.PBP all work, and the app figures out if a disc is
  PSP or PlayStation. Heavier games can be slow unless JIT is on (Technical details, at the
  bottom)." Nothing in it was a choice. JIT's own switch in TECHNICAL DETAILS already says it makes
  PSP faster, and a PSP disc is still asked about when the app can't tell.

### BIOS

- The paragraph and its wall of file names ("A few systems need a BIOS file from the real console,
  and I can't include those. Drop yours in the Files app under On My iPhone, Continuum (or import
  it with the + button), then tap the button below. If a game needs one, it'll tell you which file.
  Names it looks for: scph1001.bin, scph5501.bin, ...") became **"Put BIOS files in the Continuum
  folder in Files (or add them with +), then tap the button below. Each system below says what it
  needs."** The button is still **Install a BIOS from the Continuum folder** (the engine's launch
  messages, cBoy's options and the README all send people to it by that name).
- Each system's line is now plain text instead of typewriter text, and says it the way a person
  would:
  - "Sega CD / MISSING, will not start: needs bios_CD_U.bin or bios_CD_E.bin or bios_CD_J.bin"
    became **"Sega CD: needs bios_CD_U.bin, bios_CD_E.bin or bios_CD_J.bin"**;
  - "DOOM / found prboom.wad" became **"DOOM: ready"**;
  - "PCSX ReARMed (PS1) / found scph5501.bin" became **"PCSX ReARMed (PS1): using scph5501.bin"**;
  - "Handy (Atari Lynx) / none, optional (lynxboot.img)" became **"Handy (Atari Lynx): works
    without a BIOS (looks for lynxboot.img)"**.
  - The 32X's two files used to be two lines (a "found" could sit beside a "MISSING" for the same
    system). They are one line now: "Sega 32X: needs 32X_M_BIOS.bin and 32X_S_BIOS.bin", only the
    one still missing, or "Sega 32X: ready". This is the only place where the number of lines
    changed; the check itself (which files count, where it looks) is the same.
- Read-outs: **State** ("BIOS (pcsx_rearmed): none, HLE fallback" and so on) moved to TECHNICAL
  DETAILS → BIOS, and the full list of names the install button recognises went with it as
  **Names it looks for**, so nothing was lost: every name a system needs is also on its own line.

### SAVE STATES

- The two switches: "On, so leaving a game writes its auto-save." / "Off, so nothing is saved
  unless you save it yourself." became **"On. Leaving a game saves your spot."** / **"Off.
  Nothing's saved unless you save it."**; "On, so starting a game restores its auto-save." / "Off,
  so every game starts from the beginning." became **"On. Games open right where you left
  them."** / **"Off. Every game starts from the beginning."**
- The two long notes (why there's no save every few seconds, what turning it off keeps) became
  **"It saves when you leave a game or switch to another app. Turning it off keeps the auto-saves
  you have, and your own save slots are never touched."**
- The note at the bottom (why states live outside the Continuum folder, device backups) became
  **"Each game's own states are on its card (press and hold the game)."**
- The **Delete every game?** message now has real plurals and is shorter: "All 80 games go, with
  their save states and cheats. In-game saves are kept. This cannot be undone." (one game: "Your
  one game goes, ...").
- Read-outs: **Stored** and **Last** moved to TECHNICAL DETAILS → SAVE STATES. The section keeps
  "12 save states for 5 games", or "No save states yet".

### STORAGE

- Two long notes (the Documents directory, cue sheets naming their tracks, where artwork, cover
  lists, save states and cheats are kept and why, a file an old build left behind) became
  **"Your games are in the Continuum folder in the Files app. Don't rename a disc game's files
  there, or it won't load."**
- Read-outs: **Library** ("library: 80 game(s) of 86 file(s) in Documents") moved to TECHNICAL
  DETAILS → STORAGE. Its other three (Artwork, Cover lists, Save states) were the same lines as
  COVER ART's Stored and Cover lists and SAVE STATES' Stored, so they are under those and not
  shown twice. Deleting a game is still on its card (**Delete this game**) and a swipe in the
  All Games list.

### CORE SETTINGS

- "Each emulator core has its own settings: resolution, renderer, palettes and more. A core's list
  appears after a game has run on it once. In a game, the menu also offers settings for that game
  only." became **"Each emulator has its own settings, and they show up once you've played a game
  on it. A game's ⋯ menu can change them for that game only."**
- **TV scaling** and **TV layout (DS, 3DS)** are the TV's current choices, not read-outs, so they
  stay in the section, as plain lines ("TV scaling: Fit") instead of typewriter text.

### APP ICON

- "iPhone asks every time the icon changes. You can switch again whenever you want." became
  **"Tap one to switch. Your iPhone pops up a note each time."**; "Bitmap is set. iPhone asks each
  time, then it stays." became **"Using Bitmap. Tap another to switch."** (and, straight after a
  tap, **"Using Bitmap now."**); "Primary icon restored. iPhone asks because it has to." became
  **"Back to the Default icon."**; "This install cannot change its icon." became **"This copy of
  Continuum can't change its icon."**

### SKINS

- "3 skin(s) in the library." became **"3 skins in your library"** (and "1 skin in your
  library").
- **Button sounds**: "Plays a skin's own click (sound.caf) on every press, at the game's volume.
  Silent while the app is muted or the phone is on silent." became **"Plays a skin's own click
  when you press a button. Silent when Mute is on or your phone is on silent."**

### SYNC FOLDER

- The backup note ("Back up to Files saves one Continuum Backup .zip wherever you pick (On My
  iPhone, iCloud Drive and so on). After reinstalling, tap Restore from a backup and pick that
  .zip. A live folder that syncs by itself needs iOS to allow it, which it often does not for
  sideloaded apps.") became **"Back up to Files saves everything in one file, wherever you like.
  After a reinstall, tap Restore from a backup and pick it."**
- The live folder note (everything it keeps, when it updates, newest copy wins, Conflicts) became
  **"A live folder in iCloud Drive, Google Drive or Dropbox syncs everything by itself. iOS often
  doesn't allow it for apps from outside the App Store."**
- **Back up the games too** (five sentences about disc sizes and track files) became **"Games can
  take up a lot of space. BIOS files are never included."** It is still off unless you turn it on.
- "A save state you delete is moved to Continuum Sync/Deleted, so it is still there if you change
  your mind." became **"Save states you delete are kept in Continuum Sync, Deleted, in case you
  change your mind."**
- Read-outs: **Folder** ("none chosen" or the folder's name) and the status line under it (for
  example "sync folder: backup saved as Continuum Backup ... .zip in Downloads", "sync folder:
  restore finished: 1 skin, 12 save states. ...", or a failure) moved to TECHNICAL DETAILS → SYNC
  FOLDER, as **Folder** and **Last**. The section keeps one plain line: "Live folder: <its name>"
  when one is chosen, otherwise "Last backup: Today at 12:54 PM" (iPhone's own date wording), or
  "No backup yet". Worth knowing: a backup or restore that fails now says so only under
  TECHNICAL DETAILS (and in the activity log a feedback report sends), not in the section itself.

### CONTROLLERS, MAPPING AND MOTION

- "Tilt games (Yoshi Topsy-Turvy, WarioWare: Twisted!) read the phone's motion sensor. It only
  runs while a game asks for it. Calibrate in Controllers by holding the phone the way you play."
  became **"Tilt games like Yoshi Topsy-Turvy use your phone's motion. Calibrate it in
  Controllers, button mapping and motion."** (the button right above it; the calibrate button on
  that screen still tells you to hold the phone the way you play).
- Read-out: **Motion** ("motion: not started" and so on) moved to TECHNICAL DETAILS →
  CONTROLLERS, MAPPING AND MOTION.

## TESTING.md steps whose quoted wording changed

(For the orchestrator to reword. STATUS.md, TESTING.md and HANDOFF.md were not touched.)

- **R9** (Beetle PSX HW with no BIOS: "the BIOS line says so"): the Settings BIOS line is now in
  **Settings → TECHNICAL DETAILS → BIOS → State**. The same line is still in the player's (i)
  small text.
- **Night batch step 19** ("read the note under CORES and the two notes around SYNC FOLDER ...
  they point at buttons that really exist ("Open the game and pick it there", "Sync now") ... A
  restore line says "1 skin", not "1 skin(s)""): both quotes are out of date. The CORES note (T1)
  now says "For just one game, press and hold it and pick a core on its card."; the SYNC FOLDER
  notes now name **Back up to Files** and **Restore from a backup**, not Sync now; and the restore
  line is in **TECHNICAL DETAILS → SYNC FOLDER → Last**.
- **M1 step 3** ("Tap Sync now and wait for the line above the buttons to say it finished"): that
  line is in TECHNICAL DETAILS → SYNC FOLDER → Last now. In the section, **Sync now** reads
  "Syncing..." while it works and goes back to "Sync now" when it's done.
- **P-PSP4 (b)** ("the Folder row shows its name"): the section's first line now reads "Live
  folder: <its name>"; the Folder row itself is in TECHNICAL DETAILS → SYNC FOLDER. (Its "Choose a
  sync folder" button has been called **Live folder (advanced)** for a while.)
- **P-SYNC2** ("the line says "backup saved as Continuum Sync in ..."") and **H3** ("The status
  says files were sent"): that line is in TECHNICAL DETAILS → SYNC FOLDER → Last. The section
  shows "Last backup: ..." after a backup.
