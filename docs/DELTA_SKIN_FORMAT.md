# Making skins for Continuum

Continuum reads the same skin files as Delta and Manic EMU, so most skins made for those apps work
here too. This page is for people who make skins: what Continuum reads, and the few places where
it does its own thing.

A Delta skin is a ZIP file renamed to `.deltaskin`. Inside are an `info.json` and the pictures it
uses, all at the top level. A Manic EMU `.manicskin` is the same package with a few extra fields;
see [Manic EMU extras](#manic-emu-extras) below.

Delta documents the format itself: [Delta Custom Skins](https://noah978.gitbook.io/delta-docs/skins)
(there's a copy in [altstoreio/Delta-Docs](https://github.com/altstoreio/Delta-Docs/blob/master/skins/README.md)).

## What you can import

| File | What Continuum does with it |
| --- | --- |
| `Something.deltaskin` | Unzips it, reads `info.json`, places the buttons, draws the PDF or PNG art, and puts the game picture in each `screens` area |
| `Something.manicskin` | The same, plus Manic's function buttons, switches and `sound.caf` |
| `Something.zip` with the same contents | Treated like a `.deltaskin` |
| A bare `info.json` | The layout and the screen areas only, as there are no pictures to draw |

There are two ways in. Import skins in the skin library (Settings, Skins, Open the skin library)
takes as many files as you pick at once. Import .deltaskin, under Settings, Move the on-screen
controls, takes one skin and shows it right away.

## Packing a skin

1. Put `info.json` and every picture at the top level of the archive, with no folder around them.
2. Zip those files, then change the extension to `.deltaskin`.

Zipping the folder instead of the files inside it gives a package nothing can read.

## info.json

```json
{
  "name": "Standard GBA",
  "identifier": "com.example.gba.standard",
  "gameTypeIdentifier": "com.rileytestut.delta.game.gba",
  "debug": false,
  "representations": { }
}
```

| Field | What it's for |
| --- | --- |
| `name` | The name shown in the app |
| `identifier` | A unique reverse-DNS id for the skin |
| `gameTypeIdentifier` | Which console the skin is for (tables below) |
| `debug` | Delta's debug overlay. Continuum ignores it |
| `representations` | The layouts, per device, size and orientation |

### Which console a skin is for

A skin for a console Continuum can't play yet still imports, and it stays in your skin library.

Delta's identifiers:

| Delta | Continuum |
| --- | --- |
| `com.rileytestut.delta.game.gb` | gb |
| `com.rileytestut.delta.game.gbc` | gbc (and gb) |
| `com.rileytestut.delta.game.gba` / `.ds` / `.nes` / `.snes` / `.n64` / `.ps1` | same name |
| `com.rileytestut.delta.game.genesis` | genesis |

Manic EMU writes `public.aoshuang.game.<x>`:

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

Continuum also takes `fds` (Famicom Disk System) and `sgx` (SuperGrafx), which aren't in Manic's
list.

A skin also works on related systems: Game Boy and Game Boy Color; Mega Drive, Sega CD and 32X;
Master System, Game Gear and SG-1000; NES and Famicom Disk System; DOS and DOOM. Beyond Manic's
list, TurboGrafx-16, PC Engine CD and SuperGrafx share skins too.

If Continuum doesn't know the identifier, the skin still imports: pick the console under
"Import for" in the skin library.

### Layouts for each device and orientation

```text
representations
  iphone | ipad
    standard | edgeToEdge | splitView   (ipad uses standard / splitView)
      portrait | landscape
        assets, items, screens, mappingSize, extendedEdges, translucent
```

Continuum picks the best portrait layout and the best landscape layout separately, and uses
whichever matches how you hold the phone. The best one is the first of these that has a
`mappingSize` and at least one item: `iphone/edgeToEdge`, `iphone/standard`, `iphone/splitView`,
then the same three under `ipad`. A skin with only one orientation still imports, and that layout
is used both ways; Continuum doesn't make up the missing one.

### One layout

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

`mappingSize` is in points. Positions work the way they do in Delta: they start at the top left,
and y grows downward. Continuum places each button from the centre of its frame.

### Buttons: `items` and `inputs`

- A plain button: `"inputs": ["a"]`. List several names to make a combo that holds them all.
- A D-pad: `"inputs": { "up": "up", "down": "down", "left": "left", "right": "right" }`. The
  D-pad goes where the centre of that item's frame is.
- The button names Continuum understands (capitals, spaces, `_` and `-` don't matter): `a`, `b`,
  `x`, `y`, `l` / `l1`, `r` / `r1`, `l2` / `zl`, `r2` / `zr`, `l3`, `r3`, `select`, `start`, the
  four D-pad directions, the PlayStation's `triangle`, `circle`, `cross` and `square`, and the
  N64's `z` / `trigger` and C-buttons (`cUp`, `cDown`, `cLeft`, `cRight`). A name that matches
  one of the console's own button labels wins over these, so on the N64 "A" is the button
  labelled A. On the 3DS, `menu` and `home` are the Home button.
- An item whose inputs name a thumbstick, analog stick or circle pad (or that has a `thumbstick`
  key) becomes a real stick, not a D-pad. A right thumbstick or a C-stick becomes the right stick.
- `menu` (except on the 3DS), `quickSave` and the other function names run a function instead;
  see below. Any other name is skipped.

## Manic EMU extras

These are newer than plain Delta skins and have had less testing.

- Function buttons. An item whose `inputs` names a function runs it instead of pressing a game
  button. Continuum handles every function in Manic's list, the same way as its extra floating
  buttons.
  An item that mixes a function with any other input is turned down at import with a short
  message, as Manic's guide warns. Delta's `menu` opens a menu of every function (`flex`), except
  on the 3DS, where it stays the Home button.
- Combos. An item that names several game buttons (`["a", "b"]`) holds all of them.
- Press animation. An item's `asset.normal` is drawn as that button's own picture. Pressing it
  shows `asset.pressed` (or `highlight`) when the skin has one; otherwise the button sinks in a
  little and springs back.
- Switches. An item becomes a switch with `animation` (`type: spring`, with `begin` and `end`
  frames relative to the item's own frame), with `selfRetracting`, or with `asset.selected` and a
  single string for `inputs`. The knob moves between `begin` (off) and `end` (on), and shows
  `asset.selected` while on. A `selfRetracting` switch is only on while you hold it. Switches on
  `reverseScreens`, `volume`, `toggleControlls`, `toggleAnalog`, `tvType`, `leftDifficulty` and
  `rightDifficulty` show the real setting whenever the skin is drawn. A switch on a game button
  holds that button down while it's on.
- Button sound. A `sound.caf` at the top level of the package plays on every press, at the game's
  volume, and never while the app is muted or the phone is on silent. Settings, Skins, Button
  sounds turns it off.

## The skin library

Settings, Skins, Open the skin library lists every skin you've imported, by console. Each console
can have a default skin (or the built-in pad), and you can rename or delete a skin by swiping or
holding its row. A game's card has its own skin choice, just for that game, and in a game the ⋯
menu (or a `skins` button on the skin) switches skin on the spot and remembers it for that game.
Changes made with Edit this skin belong to that one skin.

With a skin in use, a built-in button the skin doesn't name isn't drawn at all, and the built-in
D-pad is left out when the skin has a stick or a circle pad but no D-pad.

If a skin won't import, the message says what went wrong.

## Samples

A Manic sample, [`docs/samples/manic-skin-gba-info.json`](samples/manic-skin-gba-info.json),
packed as [`docs/samples/continuum-sample-gba.manicskin`](samples/continuum-sample-gba.manicskin)
by `scripts/make-manic-skin-sample.py`. It has function buttons, a combo, a switch that stays put,
one that springs back, an item that gets turned down, and a `sound.caf`.

A small Game Boy Advance portrait `info.json` is at
[`docs/samples/delta-skin-gba-info.json`](samples/delta-skin-gba-info.json), and
[`docs/samples/continuum-sample-gba.deltaskin`](samples/continuum-sample-gba.deltaskin) packs it
with a tiny picture, so an import tries the art and the screen area together. You can also import
the bare `info.json` to try just the layout and the screen area.

## Not supported yet

- iPad and split-view layouts as their own targets. They're only used as fallbacks, in the order
  above.
- Core Image `filters`.

So far, 3DS skins (sideways, upright and with the screens swapped) and DS skins both ways round
have been checked on a phone, along with importing several skins at once. Skins for other
consoles should behave the same, but they've had less testing, so reports are welcome.

## In the code

For anyone reading the source, all in `native/ios`: `DeltaSkinImport.swift` reads the package,
`SkinLibrary.swift` maps the console identifiers (`SkinGameTypes`) and the related systems
(`SkinSharing`), `SkinScreens.swift` maps the button names (`SkinControls.slot`),
`ManicSkinItems.swift` reads Manic's extras, `SkinFunctions.swift` runs the function buttons and
`TouchControls.swift` draws the result. `scripts/check-skins.sh` checks both samples.
