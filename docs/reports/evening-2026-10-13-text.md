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

## TESTING.md steps whose quoted wording changed

(For the orchestrator to reword. STATUS.md, TESTING.md and HANDOFF.md were not touched.)

- None yet for T2's COVER ART.
