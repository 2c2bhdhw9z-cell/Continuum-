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

## TESTING.md steps whose quoted wording changed

(For the orchestrator to reword. STATUS.md, TESTING.md and HANDOFF.md were not touched.)

- **R9** (Beetle PSX HW with no BIOS: "the BIOS line says so"): the Settings BIOS line is now in
  **Settings → TECHNICAL DETAILS → BIOS → State**. The same line is still in the player's (i)
  small text.
