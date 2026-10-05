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
| `com.rileytestut.delta.game.gb` | gb |
| `com.rileytestut.delta.game.gbc` | gbc (and gb) |
| `com.rileytestut.delta.game.gba` / `.ds` / `.nes` / `.snes` / `.n64` / `.ps1` | same name |
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

Also accepted, though not in Manic's published list: `fds` (fds) and `sgx` (sgx).

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

Continuum keeps the best portrait and the best landscape it finds, each chosen separately, and
uses whichever matches how the phone is held. "Best" is the first, in this order, that has both
`mappingSize` and at least one item: `iphone/edgeToEdge`, `iphone/standard`, `iphone/splitView`,
then the same three under `ipad`. A skin with only one orientation still imports and uses that
one either way; the missing one is not invented.

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
- Game-button names Continuum maps today (`SkinControls.slot` in `SkinScreens.swift`; case,
  spaces, `_` and `-` are ignored): `a`, `b`, `x`, `y`, `l` / `l1`, `r` / `r1`, `l2` / `zl`,
  `r2` / `zr`, `l3`, `r3`, `select`, `start`, the four D-pad directions, the PlayStation
  `triangle`, `circle`, `cross` and `square`, and the N64 `z` / `trigger` and C-buttons (`cUp`,
  `cDown`, `cLeft`, `cRight`). A name that matches one of the system's own button labels wins
  over these aliases (so N64 "A" is the button labelled A). On the 3DS, `menu` and `home` are
  the Home button.
- An item whose inputs name a thumbstick, analog stick or circle pad (or that has a `thumbstick`
  key) becomes a real stick, not a D-pad; a right thumbstick or C-stick becomes the right stick.
- `menu` (outside the 3DS), `quickSave` and the other function names run a function instead
  (see Manic EMU extensions). A name that is none of these is skipped.

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

- iPad and splitView as their own targets: they are only fallbacks in the order above
- CoreImage `filters`
- Device proof beyond one case: a 3DS skin sideways shows both screens in their holes, with
  nothing over the picture, on the owner's phone (build 122, 5 October), and importing several
  skins at once works there. Upright, and DS skins, are not confirmed yet.
- A skin saved before build 109 kept only its first screen hole. Delete it and import the same
  file again; the app cannot re-read the original by itself.

Use **Import .deltaskin** in the on-screen control layout editor (one skin, previewed), or
**Import skins** in the skin library, which takes any number of files picked together. With a
skin in use, a built-in button the skin does not name is not drawn at all (and a built-in D-pad
is left out when the skin has a stick or circle pad but no D-pad). Cancelling the picker or
picking nothing leaves a clear error on that panel; a bad package names what failed.
