# cBoy, evening of 13 October 2026

Written item by item as the evening's work lands; the last item turns it into the write-up for the
owner. Everything here is **Built, untested**: nothing has been seen on a phone yet. Items 1 to 3
landed on 13 October, item 4 on 16 October.

## What landed

### 1. A Game Boy Color plays old black-and-white games in colour — Built, untested

- **For players:** the Picture setting that used to be "Colour Game Boy games" is now **"Colour for
  black-and-white games"**. **Off (like a Game Boy)** is still the default, and with it left alone
  nothing changes for anyone. **Automatic (like a Game Boy Color)** plays a black-and-white game on
  a Game Boy Color, which colours it the way the real one does: Tetris yellow and red, Pokemon Red
  red and green, Super Mario Land blue and brown, and so on for every game the real Color knows,
  with its standard colours for the rest. The twelve schemes a real Color lets you pick by holding
  buttons as it starts are there too, by name and button ("Brown (Up)" to "Reversed (Right + B)").
  cBoy's own seven looks stay, and keep their old values, so nobody's setting changes meaning.
- **How:** `compat.rs` is the Color start-up code's table and twelve button schemes, from the
  documentation (Pan Docs for how it picks, The Cutting Room Floor for the colours), with one
  documented row corrected (Radar Mission, whose two numbers that page has swapped). A Color given
  a black-and-white game now gets what its start-up code leaves: those colours in palette memory,
  the Game Boy's object order, and the registers Pan Docs lists (B holds the title sum for
  Nintendo's games; the Advance one more). Both drawers draw Game Boy mode through palette memory.
  That part is the console itself, so it also applies to someone who had already set the console
  to Game Boy Color by hand; with the setting on Off their picture stays exactly as it was.
- **With the player's own Color start-up file** (`cgb_boot.bin`, "Start from a boot ROM file" on):
  it really runs - in colour mode, reading the header and the buttons held for a picked scheme -
  and switches the console into Game Boy mode itself (KEY0), with the palettes and object order
  open to it until it hands over. A game can reach none of that afterwards.
- **Old save states:** the state format did not change (same size, same tag), so every state
  still loads. A state made before a game was in colour (on a Game Boy, or by an older cBoy) has
  blank palette memory, and on a Color it now comes back in the game's colours instead of white.
- **Mealybug: 24 → 27 of 55** - the three Color pictures that needed nothing but these colours,
  and nothing else moved.

**What the tests prove.** The table has the start-up code's exact shape (65 titles by sum, 29 by
sum and fourth letter, 14 shared sums) and every colour is one a Color can show, written as the
documentation writes it. Tetris, Pokemon Red, a shared sum split by its fourth letter both ways
(Super Mario Land against Metroid II, Pokemon Blue against Vegas Stakes), a shared sum with an
unknown letter, an unknown game and a game that is not Nintendo's each get the right colours; each
of the twelve button schemes has its own buttons and colours. The registers a Color and an Advance
leave are Pan Docs'. dmg-acid2 on a Color matches that test's own Color reference pixel for pixel
on both drawers. Through the libretro entry points: the default picture is unchanged, a Color
console with the setting left alone is unchanged, colour games are unchanged by every choice, a
console the player picked is kept, and Automatic gives the Color reference exactly. A real
start-up file (SameBoy's free one, built from its source; never committed) leaves the same palette
memory, registers and object order as cBoy's own for all 142 cartridges tried, on a Color and on
an Advance (`tests/roms.rs::a_colour_start_up_file_gives_the_same_colours`, run by hand because it
needs the file). dmg-acid2, cgb-acid2, Mooneye DMG 87/87 and Mooneye Color 7/7 runs pass on both
drawers.

**Left on purpose.** A Color's colours need the console on Auto (or a Color or Advance): a console
the player picked by hand is kept, so on "Game Boy" a game stays grey, and the setting's text says
so. Switching to one mid-game waits for the next start, like the console setting; switching back
to Off is immediate.

### 2. The Link cable setting says how to link two phones — Built, untested

- **For players:** cBoy's **Link cable** setting (Core settings, Link) no longer says linking two
  phones isn't there yet. It points to the row the app added for it (commit 1dee835), in the
  app's own words from `native/ios/PlayerScreen.swift`.
  - Before: "Unplugged is a console with nothing attached. Loopback connects the cable to itself,
    which some test programs use. Linking two phones is not available yet."
  - After: "Unplugged is a console with nothing attached. Loopback connects the cable to itself,
    which some test programs use. To link two phones, open the ... menu in a game and choose Link
    cable to a second phone." (Item 3 below then shortens it to one sentence.)
- Nothing else changed: same key, values, default, and the cable works the same.
- **What the tests prove:** a new test checks the text names the ... menu and the row "Link cable
  to a second phone" word for word, and never says "not available". The inventory regenerates
  byte for byte the same (it lists keys, not text). Full cBoy suite green.

### 3. Every cBoy setting's description is one short sentence, or none — Built, untested

- **For players:** under each of cBoy's settings there's now one short line at most. The 57
  settings a player sees went from 1,396 words to 704: the longest line went from 74 words to 22,
  and 34 of them had two to five sentences, now none does. Ten have no line at all where the name
  and choices say it (Contrast, Darken, Colour temperature, Volume, the four channel volumes,
  Advance recorded sound B, Left stick as D-pad). A few examples:
  - Game Boy Advance save chip. Before: "Auto finds it from the game. Pick one if a game does not
    save. Next start." After: "Leave it on Auto unless a game won't save; changes apply at the
    next start."
  - Sound centring. Before: "A real console's own circuit pulls the sound back to the middle,
    which keeps a steady hum out of it. Gentle leaves more of the deep bass in. Off leaves the
    sound exactly as the chip made it, hum and all." After: "Gentle keeps more deep bass, and Off
    leaves the sound raw, hum and all."
  - Colour for black-and-white games (five sentences before). After: "Automatic and the ones with
    buttons need the console on Auto or a Color, and change at the next start."
  - Link cable, after item 2: "Loopback is for test programs; to link two phones, choose Link
    cable to a second phone in a game's ... menu."
  - Contrast. Before: "Raises or lowers the contrast of the finished picture." After: no line.
- **What did not change:** every key, label, choice, default and what each setting does. A script
  compared the settings table before and after with the descriptions blanked out: identical, and
  the code around it is byte for byte the same. One group line changed too: Performance's "Speed
  and latency." is now "Speed and button delay." Picture drawing's line was only shortened (it
  still says Exact isn't finished and may look wrong); nothing about either drawer was touched.
- **The app reads one word of this text.** The engine holds a setting whose text says "restart"
  at what the game started with and shows "restart needed" with a Restart the game now button
  (`says_restart`, crates/emulator-bridge/src/cores/options.rs), and the save-state check warns
  about a state made under another value of it (`isStateSensitive`, native/ios/SaveStates.swift).
  Before and after, exactly two cBoy settings say it: Super Game Boy border and Output sample rate
  (the two cBoy itself holds until the game is loaded again). Nothing in crates/, native/, scripts/
  or .github/ quotes any old description; the only match for an old sentence is Azahar's own option
  text in its patch.
- **What the tests prove:** three new tests in options.rs. Every description is one sentence of at
  most 22 words and 125 letters, or empty only for the fourteen settings named in the test (ten
  shown, four hidden layer switches), so none can quietly disappear or grow back into a paragraph.
  Exactly Super Game Boy border and Output sample rate say "restart", and none says it in a way the
  app reads as "no restart". The nine settings that wait for the next start still say so. The
  no-jargon test now also covers the group lines and bans M-cycle, register, HLE, DMG, CGB, AGB,
  latency and scanline. Breaking the text on purpose eight ways (two sentences, a paragraph, a
  line growing back, the restart word lost, "next start" lost, the link menu lost, jargon in a
  setting and in a group line) fails the right test every time. The old "every description is
  non-empty" check moved into the new test with its named exceptions. The inventory regenerates
  unchanged (67 options), and the full cBoy suite is green: 158 tests, dmg-acid2, cgb-acid2,
  Mooneye and Mealybug as before.

### 4. The exact drawer: every test picture that has an answer — Built, untested

- **For players:** nothing changes unless you pick **Picture drawing → Exact (experimental)**;
  Standard is still the default and draws exactly as before. Exact is the drawer that copies the
  real chip a dot at a time, for the rare games that change the picture partway along a line. It
  now gets **51 of the 55** Mealybug test pictures right, up from 27, and 51 is the most there is:
  the other 4 are the test set's own "Expected result not currently available" placeholder. That
  is all 24 original Game Boy pictures and all 27 real Game Boy Color ones.
- **What changed (G2):** a game's write to one of the picture settings (scroll, palettes, the
  window, the LCD control bits) now reaches the drawer on the exact dot the console applies it,
  as SameBoy models it, and a Game Boy Color uses its own, different timing. Before, the Color
  used the Game Boy's. This alone fixed 17 Color pictures (27 → 44). The registers a game can read
  back still change at the same moment as before, so nothing a game can measure moved.
- **(G3)** On a Game Boy Color playing a black-and-white game, the drawer now follows the Color's
  own hardware rules (window start, sprite fetching, the background fetch), and the Color's
  tile-select glitch is in. Its pictures now match SameBoy's pixel for pixel on 27 of the 31
  (44 → 45).
- **(G4)** Three more rules from the hardware notes and the measurements: the rest of the Color's
  tile-select glitch (45 → 49), a sprite's first row byte taking its height setting a dot before
  it is read (50), and switching the background off right as a line's first pixel is drawn
  (51, the last black-and-white picture, four pixels away for three sessions).
- **Old save states:** the state format did not change (nothing new is saved), so every state
  still loads.
- **Mealybug: 27 → 51 of 55.**

**What the tests prove.** The full cBoy suite is green (163 tests): dmg-acid2, cgb-acid2,
Mooneye 87 of 87 and the Mooneye Color set 7 of 7 runs, on both drawers. All 51 pictures are
pinned by name, so one going wrong fails the build. New tests pin the write timing table for
both consoles, that a write leaves nothing behind once it has landed, what each step of a write
carries, and both halves of the Color's tile-select glitch (each checked to fail when its rule is
taken out). The Standard drawer was fingerprinted before and after every change, over 200 frames
of every test program and five Pokémon games, each on its own console and on a Color: same
frames, same CPU state, same save states, 336 of 336. The exact drawer was compared with SameBoy
write by write (`cores/continuum-gb/scripts/sameboy/`): on the Game Boy set every sideways-scroll,
palette and window-position write finds both drawers in the same state, and the two places cBoy's Game Boy timing
differs from SameBoy's were each tested inside a copy of SameBoy, which then passes 22 Game Boy
pictures instead of 15. Pokémon Yellow, Gold and Crystal draw all 3,000 frames tried exactly the
same on Exact as on Standard; Red and Blue differ on 5 frames, on one line of the opening fight
where the game changes the picture partway along the line.

**Left on purpose.** Exact stays experimental and never the default until the owner has seen it
on a phone. What no test here can measure is written down in
.kiro/notes/docs/CBOY_EXACT_DRAWER.md ("What is left"): five Color-only memory-access edges,
double speed, the later Color revisions (the tests are a CPU CGB C), the Super Game Boy's own
timing, and one mode-3 length case on a Color that would change the Standard drawer's timing.

## Needed in files cBoy's work does not own

- `docs/option-sources/mapping.tsv`, the `gambatte_gb_internal_palette` row (line 24): its note
  "own presets; Nintendo's built-in GBC/SGB tables are not reproduced" is out of date. Replace the
  note with: `own presets, plus the Game Boy Color's own table and its twelve button schemes; the
  Super Game Boy's built-in palettes are not reproduced`.
- `docs/option-sources/standalone.tsv`, the `useCgbColors` row (line 92): replace the note "own
  schemes, not Nintendo's table" with: `the Game Boy Color's own table (Automatic), and cBoy's own
  schemes`.
- Then rerun `scripts/build-cboy-inventory.py`. Nothing else in the inventory changes: the setting
  kept its key, so cBoy still has 67 options.
- `docs/option-sources/mapping.tsv`, the two-phone link rows (from item 2): two phones link
  through the app since commit 1dee835, so "in progress" is out of date.
  - Line 25, `gambatte_gb_link_mode`: status `gap` to `n/a`, note: `the app links two phones
    itself: the ... menu's Link cable to a second phone (Host, Join or Nearby)`.
  - Lines 26 to 38 (`gambatte_gb_link_network_port` and the twelve `..._server_ip_N`): status
    `gap` to `n/a`, note: `the app's Host shows the address and Join types it in or finds it nearby`.
  - Line 128, `tgbdual_gblink_enable`: keep `gap`, note: `two consoles on one phone: not built
    (two phones link through the app)`.
  - Then rerun `scripts/build-cboy-inventory.py`: the summary line goes from 22 gaps and 3 not
    applicable to 8 gaps and 17 not applicable. cBoy still has 67 options.
- App words cBoy's text names (from items 2 and 3), for whoever changes them: the ... menu's row
  "Link cable to a second phone" (`native/ios/PlayerScreen.swift`) is named in Link cable, and
  "Install a BIOS in Settings" (the BIOS button, now "Install a BIOS from the Continuum folder" in
  `native/ios/SettingsScreen.swift`) in three cBoy settings. If either is renamed, the cBoy text
  in `cores/continuum-gb/src/options.rs` changes with it.
- `docs/CORE_OPTIONS_STANDARD.md` section 3 suggests "Takes effect on the next restart." for a
  description. The word "restart" makes the app hold that setting until the game starts again and
  warn on save states, so worth one line there: only use it for a setting that must not change
  under a running game.
- Not done, as it would change what a setting does: the nine settings that wait for the next start
  say "next start", not "restart", so the app doesn't offer its restart button for them.

## Phone checks

Delete the old app first, as always, then install the new build.

1. **Colour for old games.** Settings, Core settings, cBoy, Picture: set "Colour for black-and-white
   games" to Automatic (leave the console setting on Auto). Open Tetris, Pokémon Red or Blue, or
   Super Mario Land. It should be in colour, like on a real Game Boy Color (Tetris yellow and red,
   Pokémon Red red and green). Set it back to Off: grey again.
2. **Pick a scheme.** Same setting, pick one of the button ones, like "Reversed (Right + B)", then
   close and reopen the game. The colours should change to that scheme.
3. **Link cable text.** cBoy's Link cable setting should say to use Link cable to a second phone in
   a game's ... menu, and every cBoy setting should have one short line at most.
4. **Exact drawer, a quick look.** Picture drawing: Exact (experimental). Play a few minutes of any
   Game Boy or Color game. It should look and play the same as Standard. Anything that looks
   broken: send a screenshot. Then switch back to Standard.
5. **Nothing changed by default.** With Picture drawing on Standard, your usual cBoy games should
   look and play exactly like before, and an old save state should still load.

