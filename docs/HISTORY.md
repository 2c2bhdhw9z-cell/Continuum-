# Continuum: the story so far

Written 10 October 2026 from the project's own records (change history, STATUS.md, TESTING.md, PRODUCT_SCOPE.md), because the old chat was lost.

## The arc

Continuum started on 17 September 2026 as a test version that ran in a web browser, to prove the core engine worked. Once it could be built for iPhone, the web version was deleted. Since then it has been one thing: an all-in-one game emulator app for iPhone, now on build 161.

## What got built and fixed (9-10 October)

- **New home screen icons:** a Settings switcher with 55 choices.
- **Network folders:** games can be loaded from a shared folder on your home network.
- **3DS:** camera, microphone and Amiibo support, plus a second fix for stuttering.
- **N-Gage / Symbian phones:** a new emulator, Continuum Symbian, added to the app.
- **NES:** Mesen 2 became the main NES emulator, with the old one kept as a backup.
- **Beta prep:** credits for the open-source parts, a GPL v3 license, and safety questions before deleting saves or cheats.
- **Full code review:** every file read line by line, with dozens of small bugs fixed. Finished 10 October.
- **PSP (builds 157-161):** fixes for the crash when opening a game, the gray half-drawn picture, black screens when resuming a save, and buzzing EA loading screens. Crash reports now show each step of a game launch.
- **3DS:** a new setting to pick how graphics get prepared.

## Decisions you made

- Everything must work without JIT (a speed boost your setup can't turn on), and it switches on by itself for anyone who can use it (7 October).
- iPhone app only. No web version, ever. Android comes later, "way down the line."
- Instructions never name a specific installer app.
- iPhone controls get the iOS glass look; Android will get its own look.
- Goal: do everything Manic EMU does, and more.
- Layout editor "done" means every control in a skin file works.

## Confirmed on your phone

- Library, importing, cover art, controls, game controllers, sound, fast forward, rewind, auto-save and resume, the game's own saves.
- DS and 3DS skins both ways round, screen swap, AirPlay to a TV.
- Mesen 2 on NES and save states (build 153).

## Built, not yet tried on a phone

- All the PSP fixes from builds 157-161, and the new 3DS setting.
- The second 3DS stutter fix and Continuum Symbian.
- 3DS camera, microphone and Amiibo; JIT; pressed-button pictures, custom buttons, editing skins in the app, touch as a mouse, the Wi-Fi transfer code.

## Partly working

PSP and 3DS overall, save slots, cheats, the layout editor, core settings, and the tester feedback button.

## Still open

- **Next:** install build 161, open a PSP game (ideally an EA one like Need for Speed) and resume its auto-save.
- Try everything in the "not yet tried" list.
- Nothing yet checks automatically that games play correctly; for now that means you playing them.
- Android, after the iPhone app is finished.
