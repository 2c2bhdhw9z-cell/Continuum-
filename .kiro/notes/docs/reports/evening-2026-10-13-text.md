# Shorter text everywhere in the app (13 and 14 October 2026)

At 6 PM on 13 October the owner sent 13 screenshots of Settings with nearly every paragraph circled
and said: "I want stuff that can be condensed. Condensed and made shorter." This is the running list
of what changed. Everything is **Built, untested**: nothing here has been on a phone yet.

Only words changed. No switch, button or choice does anything different, keeps a different setting
or starts with a different default. The technical read-outs (the small grey capitals with
typewriter-style text under them) were moved into **Settings → TECHNICAL DETAILS**, word for word,
each under the name of the section it came from.

- T1, the first half of Settings: `ec63823` (an earlier session).
- T2, the second half of Settings: 14 commits, `14f00b7` to `232ae5e`, below.
- T3, the other screens: `5cf4a2d`, `b799c7b`, `4abac4b`, `0f38af7`, `ccbff28`, `f3c66ef`,
  `a98e731`, `cbe37fe` and `86a800d` (14 October), then the sweep of everything else on
  16 October: `3fc69ad`, `46a9159` (online play and Link cable, last) and `bffba3f`. Below.
- T4, a fresh review: one lost fact put back, `49043c9`. Below.

## In short

- **Shortened:** every note in Settings, the game card, save slots, cheats and RAM search, import,
  skins and the editors, feedback, the crash and backup questions, the empty library screens,
  online play and Link cable, the lines that pop up when a game can't open (a missing BIOS, a save
  state that won't load), and the iPhone's own network and camera permission prompts. Each is now
  one or two short sentences, or gone where the control already says it.
- **Removed:** the whole PSP section of Settings; the LAYOUT note about "confusingly similar"
  names; the five controller paragraphs (now one line); file-format trivia (byte counts, `screens[]`,
  "little endian", how long a missing cover is remembered); "Search runs over the filename on
  disk"; the explanations of why things are stored where they are.
- **Read-outs:** every small grey read-out (Stored, Last, State, Folder, Now, Motion and the
  rest) moved word for word into **Settings → TECHNICAL DETAILS**, under the name of the section
  it came from. Each section kept at most one plain line ("78 covers saved (27.0 MB)", "No cheats
  yet", "Last backup: Today at 12:54 PM"). The (i) block in a game is unchanged.
- **Nothing does anything different:** no switch, button, choice, stored setting or default
  changed. Where a line's wording decides its colour (red, orange or teal, `StatusTint`), the new
  wording keeps the same colour.

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
  "No backup yet". Since T4 (`49043c9`), a restore's result, a backup that left files out, and
  anything that failed, was refused or was cancelled take that line's place, so they show in the
  section again (see T4 below).

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

### Feedback and the crash report

- Settings, FEEDBACK: "Something broken, or got an idea? Let me know here. If it's about a specific
  game, open the game and tap ⋯ then Send feedback about this game: you can rate it and draw on a
  screenshot. And if the app ever crashes, it'll offer to send a report next time you open it."
  became **"Something broken, or got an idea? Tell me here, or tap ⋯ in a game and Send feedback
  about this game to rate it and draw on a screenshot."**
- Under Send: "Opens Mail with everything filled in, going to ContinuumDev2c2b@gmail.com. You'll
  see it all before it sends. No Mail app? Pick another way to send it to that address." became
  **"Opens Mail with everything filled in, to ContinuumDev2c2b@gmail.com. No Mail app? Pick another
  way to send it there."**
- After a crash: "... It was loading a save right then, so that save won't load by itself again
  (it's still there if you want to try it). Sorry about that. Want to send a quick report? It
  really helps me fix it." became **"... It was loading a save, so that save won't load by itself
  next time (it's still there). Sorry! Want to send a quick report? It really helps."**
- The rest of the form (the toggles under SENT WITH IT, the placeholders) was already short and
  casual, and is unchanged.

### The backup question ("Save a backup file now?")

- "Deleting Continuum deletes everything in it. If you ever reinstall, tap Restore from a backup and
  pick this file. "With games" puts your games in too, which can take a lot of space." became
  **"Deleting Continuum deletes everything in it. This file brings it all back (Settings, Restore
  from a backup), and with games it takes more space."** The title and the three buttons (Back up
  with games, Back up without games, Not now) are unchanged, and it still asks at the same times.

## T3, the sweep of everything else (16 October)

How: every string over about 120 characters and every line of three sentences or more in
`native/ios`, plus the iPhone permission prompts in `Info.plist` and `project.yml`, read where it
shows, outside TECHNICAL DETAILS and (i). Each is **Built, untested**.

### Shortened (`3fc69ad`)

- **A game that needs a BIOS** (since `ec644be` this pops up just above the tabs when you tap the
  game). "Sonic CD needs bios_CD_U.bin, a Mega-CD / Sega CD BIOS (U is USA, E is Europe, J is
  Japan), which Sega CD cannot start without and which cannot ship with the app. Put it in the
  Continuum folder in Files, then use Settings, Install a BIOS from the Continuum folder" became
  **"Sonic CD cannot start without bios_CD_U.bin, a Sega CD BIOS (U is USA, E is Europe, J is
  Japan). Put it in the Continuum folder in Files, then tap Install a BIOS from the Continuum
  folder in Settings."** The same shape for the Disk System ("disksys.rom, the Disk System's
  startup file"), PC Engine CD and DOOM ("prboom.wad, DOOM's data file, which the app normally
  adds by itself"). Saturn on Beetle Saturn: three sentences became **"... cannot start on Beetle
  Saturn without sega_101.bin or mpr-17933.bin, but Yabause can. Put one in the Continuum folder
  in Files, then tap Install a BIOS from the Continuum folder in Settings."** The PlayStation line
  can't show today (Beetle PSX HW starts on its own built-in BIOS), but it lost "Beetle has no
  HLE" the same way. Gone from all of them: "cannot ship with the app" and "It was not found in
  the system folder".
- **A save state that won't load.** "The core has been rebuilt since this state was saved (mgba
  0.10.2 wrote it, this build is 0.10.3), so its internal layout may have moved. Not loading it,
  to avoid corrupting the game." became **"This state is from another version of mgba (0.10.2,
  this one is 0.10.3). It isn't loaded, to keep your game safe."** Different settings: three
  sentences became **"This state was saved with different emulator settings (Model), and loading
  it now could crash the game. Set them back to load it."** The auto-save that crashed the app:
  **"Started Tetris fresh: its auto-save crashed the app last time, so I skipped it once. It's
  still there."** An auto-save from an emulator the app no longer has now ends **"It won't load,
  because this version doesn't have its emulator any more, but your in-game saves are fine."**
  The PSP resume that gives up: **"Could not pick up <game> where you left off. Your auto-save is
  safe, and this session won't save over it."** Exporting a battery save a game doesn't have:
  **"Tetris has no battery save yet. Save in the game's own menu first; not every game can."**
- **The game card's SAVE STATES note:** "A state goes back into the emulator that made it, so
  changing that emulator's settings can stop one loading. Most emulator updates keep old states
  working, and when one can't load we say why instead of risking your game." became **"Changing
  an emulator's settings can stop a state loading. If one can't load, it says why."**
- **Choosing a cover** (on the card): "This app has no thumbnail directory for Flash, so there is
  nothing to choose from. An image from Files still works." became **"There are no covers online
  for Flash, but an image from Files still works."**; "Back to automatic artwork for this game.
  It keeps its generated plate until cover lookups are turned back on in Settings." became **"Back
  to the automatic cover. It shows a title card until you turn Look up cover art back on in
  Settings."** (the switch's real name); when some cover lists don't download: **"Found 3 covers,
  but some cover lists didn't download (<why>). Try again when you're online for more."**; a
  download that isn't a picture: **"... wasn't a picture the app could read, so the cover didn't
  change"**.
- **Flash and J2ME games:** "save states are not available for Flash games: Ruffle cannot freeze
  a running movie. The game's own saves are kept automatically" became **"Flash games cannot use
  save states, but their own saves are kept automatically"**; "... they run in Ruffle, not a
  libretro core. Keys and save files are under the player menu, Flash settings" became **"Flash
  games have no core settings. Their keys and save file are in Flash settings, the sliders button
  at the top"**; the player stopping is **"... the player stopped, usually from running low on
  memory. Quit and open the game again; the last automatic save is kept"**; a build without the
  player is **"... this build is missing the Flash player, so Flash games cannot open until the
  next one"** (the build-machine detail went).
- **Importing a PDF manual** under a name that's taken: **"manual Zelda.pdf saved in Manuals as
  Zelda copy.pdf, since another Zelda.pdf was already there. Attach it on the game's card, or
  rename it in Files."** (it was one 220-character sentence).
- **Settings, CORES:** "Not sure? Keep the first one. Save states only load on the core that
  made them." became **"If you're not sure, keep the first one. Save states only load on the
  core that made them."** **Delete every game?** is two sentences now: **"All 80 games go with
  their save states and cheats, and this can't be undone. In-game saves are kept."**
- **The iPhone's own questions.** Local network: "Continuum uses the local network for two-player
  online play with another phone, for Wi-Fi transfer of games and saves from a computer's
  browser, and to browse a NAS or computer over WebDAV or SMB file sharing." became **"Continuum
  uses your local network for online play with a second phone, Wi-Fi transfer from a computer,
  and shared folders on a computer or NAS."** Camera: it names the Game Boy Camera now, **"Games
  that use a camera, like the Game Boy Camera or 3DS camera games, can see it while you play.
  Only used when Allow camera in games is on."**

### Online play and Link cable, last, after a fresh rebase (`46a9159`)

- Link cable: "Like two Game Boys with a cable between them. Each phone plays its own game and
  keeps its own save, so you can trade, battle or play head to head, even Red with Blue. Both
  phones need cBoy for Game Boy games (Settings, Cores). Rewind, fast forward, pause, reset and
  loading a state are off while the cable is on." became **"Each phone plays its own game and
  save, so you can trade or battle, even Red with Blue. Both need cBoy (Settings, Cores), and
  rewind, fast forward, pause, reset and loading a state are off while it's plugged in."**
- Online play: three sentences ("Both phones need the same ROM file and the same Continuum
  build ...") became **"Both phones need the same game file and Continuum version, with cheats
  off. Joining picks up the host's game as it is, and rewind, fast forward, reset and loading a
  state are off while you play."**
- Under the Host button: "The other phone joins with the address shown here, or finds this phone
  under Nearby." (no address shows until you host) became **"Then the other phone types the
  address that shows up, or picks this phone under Nearby."**, plus **"More delay hides lag; 2
  suits home Wi-Fi."** for online play.
- The status lines on these screens were already short. The two Settings notes were done in T2.

### The crash question (`bffba3f`)

- "... Sorry! Want to send a quick report? It really helps." became **"... Sorry! A quick report
  really helps."** The buttons (Send a report, No thanks) ask the question.

### Left as they are, on purpose

- The lowercase status lines that start with what they're about (`artwork:`, `audio:`, `touch
  layout:`, `controls:`, `library:`, `restore:`). They are the app's read-outs: TECHNICAL
  DETAILS, (i) and the activity log a feedback report sends. The long ones are the cover sweep's
  own notes, which show nowhere else.
- Read-outs already under TECHNICAL DETAILS (Rewind holds, Attached), and the NOT WIRED YET note,
  whose section never shows (its list is empty).
- Two-sentence notes of 120 to 165 characters that already read like a person talking, for
  example "Your controller and the on-screen pad both work, and up to four controllers can play at
  once. Pair one in your iPhone's Bluetooth settings."
- From T4's notes: **Retry failed lookups** and **Forget the downloaded cover lists** still answer
  only under TECHNICAL DETAILS → COVER ART → Last. Nobody loses a choice by that: the covers
  come back or download again by themselves. Also: which core needs a BIOS is no longer in CORES.
  Left, because nobody loses it: BIOS lists each core that needs a file ("Beetle Saturn: needs
  sega_101.bin or mpr-17933.bin"), and tapping a Saturn game on Beetle Saturn without one says
  Yabause can start it.

Checked: `swiftc -frontend -parse` on every changed Swift file before each push;
`scripts/check-players.sh` (it compiles `WebPlayerCore.swift` and checks every Flash and J2ME line
still names the game kind): 0 failed; `Info.plist` loads as a property list. Every reworded status
line was run through the same words `StatusTint` uses, and each keeps its colour (red, orange or
teal), so what pops up, how long it stays and the dot's colour are as before.

## T4: the fresh review (16 October)

A reviewer who wrote none of T1 to T3 found every changed file parses and every interpolation is
intact. One fact a player needed had gone: after the read-outs moved, a restore's result (with its
one step left, "close Continuum fully and reopen it") and a failed backup showed only under
TECHNICAL DETAILS. `49043c9` puts them back as SYNC FOLDER's one plain line, for example
**"Restore finished: 1 skin, 12 save states. Close Continuum fully and reopen it once to apply the
rest of your settings."** Everyday lines (a backup that went fine) still leave "Last backup: ..."
there.

## Phone checks (for TESTING.md, in the owner's words)

1. Scroll down **Settings**. Every note under a switch or button is one or two short sentences, and
   there's no PSP section.
2. Scroll to **Technical details** at the bottom. The small grey read-outs from the other sections
   (Cover art, BIOS, Save states, Storage, Sync folder, Online play and the rest) are all there,
   each under its section's name.
3. Press and hold a game to open its card. The notes under Save states, Cheats and Cover art are
   short and read like a person wrote them.
4. Only if you have a Sega CD, PC Engine CD or Disk System game: tap it before you put its BIOS
   back. A line pops up just above the tabs, two sentences, naming the file and telling you to tap
   **Install a BIOS from the Continuum folder** in Settings.
5. In a game, tap **⋯** → **Play online with a second phone** (or **Link cable to a second phone** in
   a Game Boy game on cBoy) and read the note at the bottom. Two short sentences.
6. The first time you open **Play online** or Wi-Fi transfer, your iPhone asks about the local
   network. Its question is one short sentence.
7. Only if you can spare a backup: **Settings → Sync folder → Restore from a backup**. When it's
   done, the line under SYNC FOLDER says what came back and to close Continuum and open it again.

## TESTING.md steps whose quoted wording changed

Every quoted line and every bold button or section name in TESTING.md was checked against the
app's current text on 16 October. These are the ones that no longer match (for the orchestrator to
reword; STATUS.md, TESTING.md and HANDOFF.md were not touched). The Home screen job's five (lines
63, 69, 762-763, 785 and 792) are in `evening-2026-10-13-library.md`.

- **Night batch step 18** ("read the **SAVE STATES** paragraph ... two short sentences ... doesn't
  claim a state needs the same build"): still passes as written; the paragraph now reads "Changing
  an emulator's settings can stop a state loading. If one can't load, it says why."
- **Night batch step 19**: old: they point at buttons that really exist ("Open the game and pick
  it there", "Sync now") ... A restore line says "1 skin". New: the CORES notes read "Kicks in
  from the next game you open. For just one game, press and hold it and pick a core on its card."
  and "If you're not sure, keep the first one. Save states only load on the core that made them.";
  the SYNC FOLDER notes name **Back up to Files** and **Restore from a backup** (**Sync now** is only
  there once a live folder is chosen); a restore's line shows right under SYNC FOLDER and says
  "Restore finished: 1 skin, ...".
- **Night batch step 26** and **R10**: old: "The second tap says one is already running" / "The
  second tap says a sync is already running". New: while a backup runs, **Back up to Files** reads
  "Working..." and both buttons go dim, so there's no second tap to make; the "already running"
  line is only under TECHNICAL DETAILS → SYNC FOLDER → Last.
- **R9**: "the BIOS line says so". That line is in **Settings → TECHNICAL DETAILS → BIOS → State**,
  and still in the (i) small text in the game. In the BIOS section, the row reads "Beetle PSX HW
  (Vulkan): works without a BIOS (looks for scph5501.bin, ...)".
- **M1 step 1**, **P-PSP4 (b)** and **H3**: old: **Choose a sync folder**. New: **Live folder
  (advanced)** ("Choose a different live folder" once one is chosen). P-PSP4's "the Folder row
  shows its name" is now the line "Live folder: <its name>"; the Folder row is under TECHNICAL
  DETAILS → SYNC FOLDER.
- **M1 step 3**: old: "wait for the line above the buttons to say it finished". New: **Sync now**
  reads "Syncing..." while it works and goes back to "Sync now"; the finished line is under
  TECHNICAL DETAILS → SYNC FOLDER → Last.
- **P-SYNC2**: old: the line says "backup saved as Continuum Sync in ...". New: under SYNC FOLDER it
  says "Last backup: Today at ..."; the saved line ("backup saved as Continuum Backup ... .zip in
  ...") is under TECHNICAL DETAILS → SYNC FOLDER → Last. (A restore now picks that .zip, not a
  "Continuum Sync" folder.)
- **H3**: old: "The status says files were sent". New: that line is under TECHNICAL DETAILS → SYNC
  FOLDER → Last.

Nothing else in TESTING.md quotes text this job changed. Still right as written: C6 and C15 (the
BIOS lines still name the exact file), K1 and "Next install" step 5 (the started-fresh line still
says which slot the old auto-save is in), J3's "Makes PSP, 3DS, PlayStation, N64 and Dreamcast
faster", O1's "Save a backup file now?" and **Back up without games**, Q1's **Reset every system's
controls and skins**, H1's **Play online with a second phone**, and A5's **Cheats and RAM search...**.
