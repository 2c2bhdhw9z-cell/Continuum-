# Delta and Manic EMU skin packages (Continuum import)

Continuum imports controller layouts from the documented **Delta** skin package format:
a ZIP archive renamed to `.deltaskin`, containing a flat `info.json` plus image assets.
Manic EMU's `.manicskin` is the same package with extra item fields; see
[Manic EMU extensions](#manic-emu-extensions) below.

Source of truth for the schema: [Delta Custom Skins](https://noah978.gitbook.io/delta-docs/skins)
(also mirrored in [altstoreio/Delta-Docs](https://github.com/altstoreio/Delta-Docs/blob/master/skins/README.md)).

## What you drop in

| Drop | What Continuum reads today |
| --- | --- |
| `Something.manicskin` | Same as `.deltaskin`, plus Manic's functions, switches and `sound.caf` |
| `Something.deltaskin` | Unzips, reads `info.json`, maps buttons, loads PDF/PNG art, applies `screens` |
| `Something.zip` with the same contents | Same as `.deltaskin` |
| Bare `info.json` | Layout + screens only (no ZIP assets to draw) |

Import applies hitbox positions from `items`, draws `assets` PDF/PNG behind the pad, and
places the game picture in every `screens[].outputFrame`.

## Package rules (from Delta docs)

1. Put `info.json` and all asset files **flat** in the archive (no parent folder).
2. Zip those files, then rename the extension to `.deltaskin`.
3. Zipping the containing folder instead of the files themselves makes the package invalid.

## `info.json` (required top level)

```json
{
  "name": "Standard GBA",
  "identifier": "com.example.gba.standard",
  "gameTypeIdentifier": "com.rileytestut.delta.game.gba",
  "debug": false,
  "representations": { }
}
```

| Field | Role |
| --- | --- |
| `name` | Display name |
| `identifier` | Unique reverse-DNS id for the skin |
| `gameTypeIdentifier` | Which console the skin belongs to (table below) |
| `debug` | Delta overlay flag; Continuum ignores it |
| `representations` | Device / size / orientation trees |

### `gameTypeIdentifier` values Continuum recognises

Mapped by string in `SkinLibrary.swift` (`SkinGameTypes`), so a skin for a console this build
has no core for yet is still imported, listed and kept.

| Delta | Continuum |
| --- | --- |
| `com.rileytestut.delta.game.gbc` | gbc (and gb) |
| `com.rileytestut.delta.game.gba` / `.ds` / `.nes` / `.snes` / `.n64` | same name |
| `com.rileytestut.delta.game.genesis` | genesis |

Manic writes `public.aoshuang.game.<x>`:

| `<x>` | Continuum | `<x>` | Continuum | `<x>` | Continuum |
| --- | --- | --- | --- | --- | --- |
| wsc | wswan | 7800 | atari7800 | ms | sms |
| flash | flash | 5200 | atari5200 | gg | gg |
| wii | wii | 2600 | atari2600 | sg1000 | sg1000 |
| ngc | gamecube | arcade | arcade | psp | psp |
| amiga | amiga | dc | dreamcast | 3ds | n3ds |
| c64 | c64 | ps1 | ps1 | ds | ds |
| ngp | ngp | pm | pokemini | gba | gba |
| pce | tg16 and pcecd | vb | vb | gbc | gbc |
| symbian | symbian | n64 | n64 | gb | gb |
| dos | dos | ss | saturn | nes | nes |
| j2me | j2me | md | genesis | snes | snes |
| doom | doom | mcd | segacd | | |
| jaguar | jaguar | 32x | sega32x | | |
| lynx | lynx | | | | |

A skin also fits its related systems: GB and GBC; Mega Drive, Sega CD and 32X; Master System,
Game Gear and SG-1000; NES and FDS; DOS and DOOM (and, beyond Manic, the PC Engine family).

Unknown identifiers still import: pick the console under "Import for" in the skin library.

### Representation tree

```text
representations
  iphone | ipad
    standard | edgeToEdge | splitView   (ipad uses standard / splitView)
      portrait | landscape
        assets, items, screens, mappingSize, extendedEdges, translucent
```

Continuum prefers, in order: `iphone/edgeToEdge/portrait`, `iphone/standard/portrait`,
then the first orientation that has both `mappingSize` and `items`.

### One orientation object

```json
{
  "assets": { "resizable": "iphone_edgetoedge_portrait.pdf" },
  "mappingSize": { "width": 414, "height": 896 },
  "items": [
    {
      "inputs": ["a"],
      "frame": { "x": 313, "y": 540, "width": 47, "height": 47 }
    }
  ],
  "screens": [],
  "extendedEdges": { "top": 10, "bottom": 10, "left": 10, "right": 10 },
  "translucent": false
}
```

`mappingSize` is in **points**. Continuum converts each item's frame centre to a fraction
of that size and writes Continuum layout fields (`buttonFrees`, D-pad / SELECT / START
centres). Coordinates use the Delta convention: origin top-left, y increases downward.

### `items` / `inputs`

- Simple button: `"inputs": ["a"]` (several names make a combo that holds them all).
- D-pad / stick: `"inputs": { "up": "up", "down": "down", "left": "left", "right": "right" }`
  — Continuum places the D-pad cluster from that item's frame centre.
- Names Continuum maps today: `a`, `b`, `x`, `y`, `l`, `r`, `l2`, `r2`, `select`, `start`,
  and the four D-pad directions. Delta-only names (`menu`, `quickSave`, `z`, C-buttons, …)
  are functions now (see Manic EMU extensions); a name that is neither is skipped.

## Manic EMU extensions

Read by `ManicSkinItems.swift`, drawn by `TouchControls.swift`.

- **Function buttons.** An item whose `inputs` names a function runs it instead of a game
  button. Every function in Manic's list is handled by one dispatcher
  (`SkinFunctions.swift`), the same one the extra floating buttons use. A function together with
  any other input on one item is refused at import with a status line, as Manic's guide warns.
  Delta's `menu` opens the all-functions menu (`flex`), except on the 3DS, where it stays Home.
- **Combos.** An item naming several game buttons (`["a", "b"]`) holds all of them.
- **Press animation.** An item's `asset.normal` is drawn as that button's own layer. A press
  swaps in `asset.pressed` (or `highlight`) when the file has one; otherwise the layer is pushed
  in (scaled and dimmed) and springs back.
- **Switches.** `animation` (`type: spring`, `begin` and `end` frames relative to the item's own
  frame), `selfRetracting`, or `asset.selected` with `inputs` as a single string make an item a
  switch. Its picture (the knob) moves between `begin` (off) and `end` (on), showing
  `asset.selected` when on. A momentary (`selfRetracting`) switch is on only while held. A switch
  on `reverseScreens`, `volume`, `toggleControlls`, `toggleAnalog`, `tvType`, `leftDifficulty` or
  `rightDifficulty` reads the real state whenever the skin is laid out. A switch on a game button
  holds that button while it is on.
- **Button sound.** `sound.caf` in the package root plays on every press of a skin control, at
  the game's volume, never while the app is muted or the phone is on silent. Settings, SKINS,
  "Button sounds" turns it off.

## The skin library

Settings, SKINS, "Open the skin library": every imported skin per system, the default per
system (or the built-in pad), rename and delete (swipe or long-press a row). A game's card has
a SKIN choice for that game alone, and the player's ... menu (or a `skins` button) switches skin
mid-game, kept for that game. Edits made in the skin editor belong to one skin.

## Sample

A Manic sample, [`docs/samples/manic-skin-gba-info.json`](samples/manic-skin-gba-info.json)
packed as [`docs/samples/continuum-sample-gba.manicskin`](samples/continuum-sample-gba.manicskin)
by `scripts/make-manic-skin-sample.py`, has function buttons, a combo, a latching and a momentary
switch, a refused item and a `sound.caf`.

A minimal GBA portrait `info.json` lives at
[`docs/samples/delta-skin-gba-info.json`](samples/delta-skin-gba-info.json). A packed
[`docs/samples/continuum-sample-gba.deltaskin`](samples/continuum-sample-gba.deltaskin)
includes a tiny PNG so Import can exercise art + `screens` together. You can also import
the bare `info.json` to exercise layout/screens without ZIP assets.

## What still does not work

- Landscape / iPad / splitView selection beyond the preference order above
- CoreImage `filters`
- Multi-screen DS layouts beyond using the first `screens[]` entry for the picture hole
- Device proof of the import path (code only until a build is tried on a phone)

Use **Import .deltaskin** in the on-screen control layout editor, or the skin library. Cancelling the picker or
picking nothing leaves a clear error on that panel; a bad package names what failed.
