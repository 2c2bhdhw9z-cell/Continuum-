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
  folder in Files (or add them with +), then tap the button below. Each system under it says what
  it needs."** The button is still **Install a BIOS from the Continuum folder** (the engine's launch
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

### ONLINE PLAY

- "Two players on two phones. Load the same game on both, then open the … menu in the player and
  choose Play online. One phone hosts and shows its address; the other types it or picks it under
  Nearby. Same Wi-Fi works out of the box; over the internet the host's router must forward TCP
  port 55435." became **"Open the same game on both phones, then tap ⋯ and Play online with a
  second phone. Over the internet, the host's router needs TCP port 55435 open."** (the menu row's
  real name; the screen it opens already shows the address and Nearby).
- "Game Boy and Game Boy Color games on cBoy get a link cable instead: each phone plays its own
  game, so you can trade Pokemon or play Tetris head to head. Choose Link cable in the same …
  menu." became **"Game Boy and Color games on cBoy get Link cable to a second phone there
  instead, so you can trade Pokémon or battle."** (again the row's real name).
- Read-outs: **Now** and **Input delay** moved to TECHNICAL DETAILS → ONLINE PLAY.

### TECHNICAL DETAILS

- **Use JIT when it's available**: "Makes PSP, 3DS, PlayStation, N64 and Dreamcast faster. If a
  game acts up with it, turn this off. Kicks in from the next game you open." became **"Makes PSP,
  3DS, PlayStation, N64 and Dreamcast faster, from the next game you open. Turn it off if a game
  acts up."** (It still starts with the words TESTING.md J3 quotes.)
- **Apple performance overlay**: "Apple's FPS / GPU box drawn over the game. Off hides it. If it is
  still showing, close Continuum fully (swipe it away) and open it again." became **"Apple's FPS
  box over the game. If it's still there after you turn this off, close Continuum fully and open
  it again."**
- **Finish setting JIT up** (only on a phone that needs it): "Only works if you turned JIT on with
  StikDebug and picked the universal script. If you used the button above, this is already done
  for you." became **"Only needed if you turned JIT on in StikDebug yourself, with the universal
  script."** (the old second sentence pointed at a button that isn't always there).
- **Show technical stuff**: "FPS and what the app is doing behind the scenes. You don't need this
  to send feedback, it's attached for you." became **"FPS and what's going on behind the scenes.
  Feedback already includes it."**
- The JIT line itself, the StikDebug buttons and the whole technical block are as they were. The
  section now ends with every read-out moved out of the other sections, under their names, in
  screen order: SPEED AND REWIND, GAME CONTROLLERS, ON-SCREEN CONTROLS, CHEATS, RETROACHIEVEMENTS,
  MICROPHONE, CAMERA AND AMIIBO (from T1), then COVER ART, BIOS, SAVE STATES, STORAGE, SYNC
  FOLDER, ONLINE PLAY and CONTROLLERS, MAPPING AND MOTION.

### ABOUT

- "Continuum is still in beta, so expect some rough edges. Games and BIOS files aren't included,
  you'll need your own. If something breaks, Send feedback at the top of Settings goes straight to
  me." became **"Continuum is still in beta, and games and BIOS files aren't included. If something
  breaks, Send feedback at the top of Settings goes straight to me."**
- The open-source credits' note: "Continuum is built on these free emulators and libraries. Each
  one is the work of its own authors and keeps its own license. Tap a name to see its source
  code." became **"Continuum is built on these free emulators and libraries, each with its own
  authors and license. Tap a name to see its source code."** Every credit and licence line is
  untouched.

## T3: the other screens

### The game card (press and hold a game)

- An incomplete disc: "This cue sheet is incomplete: 1 track missing. A PlayStation game is a .cue
  plus every .bin track it names, so import the missing tracks with the same names and it will
  boot." became **"This disc isn't complete (1 track missing). Import its .bin files too, with the
  same names, and it'll start."**
- No save states: "No states for this game yet. The save button at the top of the player writes
  one, and leaving the game or putting the app in the background writes a separate auto-save that
  this game picks up from next time." became **"No save states yet. In the game, tap ⋯ and Save to
  the next free slot."** (the old one pointed at a save button the player no longer has; saving is
  in the ⋯ menu now). The SAVE STATES paragraph under it was already two short sentences from the
  night batch and is unchanged.
- CHEATS: three paragraphs (how codes are stored, every system's code style, how cheats reach the
  emulator) became **"To find your own cheats, play the game and open Cheats and RAM search from
  the ⋯ menu. A code that does nothing is usually for another version of the game."**
- COVER ART: "A cover chosen here wins over anything found automatically, including after a
  relaunch, and an image from Files or a frame captured from the game wins over everything, ...
  The generated plate is always underneath, so nothing is ever blank." became **"A cover you pick
  here, an image from Files or a frame from the game always wins over the automatic one."**
- "Look in other systems for more covers": the paragraph about four other consoles and nine lists
  became **"Lots of games came out on other consoles too. The first look can download about 11 MB
  of cover lists."** (the download size is kept: it helps you decide).
- Taking a cover from the game: the paragraph (which said to press and hold the save button)
  became **"You can also use a frame from the game: while it plays, tap ⋯ and Use this frame as
  the cover. Pause first to catch the exact moment."**
- While it searches: "Looking through the server's own cover lists. Any this device does not
  already have is downloaded once and kept for a month." became **"Looking through the cover
  lists. Any your phone doesn't have yet are downloaded once."**
- The Source line with no cover: "no cover stored yet, so the generated plate is showing" became
  **"no cover yet, so it shows a title card"** (and the same for lookups off).
- **Delete this game**: "Deletes the file from the Continuum folder. If this is a cue sheet, the
  .bin tracks it named stay on disk: working out which tracks belonged to it means parsing the
  sheet, and a wrong guess deletes a track another game needs." became **"Deletes the game with its
  save states and cheats. In-game saves are kept, and a disc game's .bin files stay in the
  Continuum folder."** (what the button already did, said plainly).
- SKIN: "This game's own skin. The system default applies when this is on default." became **"Pick
  a skin just for this game, or leave it on the system default."**
- Kept as they are: the facts at the top (On disk as, Detail, Runs on), which are the game's own
  details, and the short core, manual and achievements lines.

### Cheats and RAM search (the game card, and ⋯ → Cheats and RAM search...)

- The line about which codes work: "Works with: Game Genie, ... For a multi-line code, put it all
  in one box with + between the lines. Codes are made for one exact version of a game, and the
  wrong one can freeze it. If that happens, switch the cheat off and load an earlier save." became
  **"Works with: Game Genie, ... Join a longer code's lines with +, and if a code freezes the game,
  turn it off and load an earlier save."**
- For an emulator that ignores codes: "Heads up: this system's emulator ignores typed codes
  (GameShark, Action Replay and so on), so they won't do anything here. The RAM search below can
  still make cheats for most games." became **"Heads up: this system's emulator ignores typed
  codes, but the RAM search can still make cheats for most games."** (on the card the RAM search
  isn't "below", it's in the game).
- RAM SEARCH: the paragraph (including "Values are read little endian") became **"Start a search,
  change the thing you're after in the game (lose a life, spend money), then come back and filter.
  A few rounds usually leave one address."**

### Save slots (the card's Save slots, export and import, or ⋯ → Save slots...)

- MORE STATES: "This game had more than 50 states before slots had a fixed number, so these are kept
  here rather than deleted. They load, export and delete like any other; free a slot to have the
  next save go there." became **"These are from before there were 50 slots. They load, export and
  delete like any other."**
- FILES: "A state file carries the core and core version that wrote it, so an imported state is
  checked exactly like one saved here and is refused with the reason if the core differs. The
  battery save is the game's own save, the one made from its menu; it is the .srm file other
  emulators use, and it is kept automatically whenever you leave the game." became **"An imported
  state only loads on the emulator that made it. The battery save is the game's own save from its
  menu, the .srm file other emulators use."**
- The small line at the top of the sheet (what the last save, load or import did) is unchanged: it
  is the sheet's answer to each tap.

### Import (the + button) and what an import says

- On this phone: "Files also reaches iCloud Drive, Google Drive, Dropbox and OneDrive: install
  their app and they appear under Browse in the Files picker. The clipboard takes files copied in
  Files, or on a Mac with Handoff." became **"Files also reaches iCloud Drive, Google Drive,
  Dropbox and OneDrive, once their apps are on your phone."**
- From a computer: "Type the address into a browser on the same Wi-Fi to send games and saves, see
  the library, and download battery saves. It stops when you leave the app." became **"Open the
  address in a browser on the same Wi-Fi to send games and saves. It stops when you leave the
  app."** (The line under the address, "Type this whole address, including the last part...", is
  unchanged: TESTING B1 relies on it.)
- Also: "Drag files from another app onto the library, or use Share or Open in from Files, Mail and
  Safari. Games, .deltaskin and .manicskin skins, .cht cheats, save files and PDF manuals are all
  recognised." became **"You can also drag files onto the library, or use Share or Open in from
  Files, Mail or Safari. Games, skins, cheats, saves and PDF manuals all work."**
- Which system?: "Continuum could not tell which system this is for: <why>. Pick one and it is
  remembered for this file." became **"Continuum couldn't tell which system this is (<why>). Pick
  one and it'll remember."**
- A skin that names no console used to say to import it from "Settings, Layout, Import .deltaskin"
  (no such place). It now says **"... the skin doesn't say which console it's for. Import it in
  Settings, Skins, Open the skin library, and pick the console"**.
- A save file for no game in the library: "... so the save has nowhere to go. Open the game's
  card, Save slots, Import a save file" became **"... no game in the library has that name. Open
  the game's card and use Save slots, export and import"** (the card's real button).

### The library's empty screens

- Nothing imported yet: "Tap Import games and pick your game files (zips work too). For disc games
  with a .cue and .bin files, select the .cue and all the .bins at once. You can also drop files
  into the Files app under On My iPhone, Continuum and they'll show up here." became **"Tap Import
  games and pick your games (zips work too), or drop them in the Continuum folder in Files. For a
  disc game, pick the .cue and all its .bin files together."**
- No favorites yet: "Press and hold a game to open its detail sheet and tap the star, or swipe a
  row right in All Games. Favorites are remembered by the game's file name, so they survive a
  relaunch, and a file that is temporarily missing is not forgotten." became **"Press and hold a
  game and tap its star, or swipe a game right in the All Games list."**
- No match: "... Search runs over the filename on disk and the title derived from it." is gone;
  it just says **"Nothing in the library matches "<what you typed>"."**

### Skins, the skin editor and the controls editor

- The skin library's footer: "A skin for one system also fits its relatives: Game Boy and Game Boy
  Color; Mega Drive, Sega CD and 32X; Master System, Game Gear and SG-1000; NES and Famicom Disk
  System; DOS and DOOM." became **"A skin also fits its system's relatives, like Game Boy and Game
  Boy Color, or Mega Drive, Sega CD and 32X."**
- After an import: "Imported 1 skin(s): ..." became **"Imported 1 skin: ..."** (and "Imported 3
  skins: ..."); "does not say which console it is for" became "doesn't say which console it's
  for".
- The skin editor: "Tap an outline to select it. Drag it to move, drag its corner to resize. Blue
  is a button, orange a function button, green a stick, red a screen hole." became **"Tap an
  outline to pick it, drag to move, drag a corner to resize. Blue is a button, orange a function,
  green a stick, red the game screen."**; "Edits are kept on top of the imported skin, which is
  never changed. Importing the skin again starts from the file." became **"Your edits never
  change the skin file itself."** (the Reset ... to the file button is right above it).
- Extra buttons in the controls editor: "None yet for Game Boy in portrait. Added buttons appear in
  the middle of the screen: drag one to place it, pinch around it or use the size slider to
  resize." became **"None yet for Game Boy in portrait. New ones appear in the middle: drag to
  place, pinch or use the size slider to resize."**
- The controls editor had no other long text. The Skin sheet in a game ("Kept for <game>. Change
  the default for every <system> game in the skin library.") was already two short sentences.

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
