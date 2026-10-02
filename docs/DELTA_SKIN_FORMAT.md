# Delta skin packages (Continuum import)

Continuum imports controller layouts from the documented **Delta** skin package format:
a ZIP archive renamed to `.deltaskin`, containing a flat `info.json` plus image assets.

Source of truth for the schema: [Delta Custom Skins](https://noah978.gitbook.io/delta-docs/skins)
(also mirrored in [altstoreio/Delta-Docs](https://github.com/altstoreio/Delta-Docs/blob/master/skins/README.md)).

## What you drop in

| Drop | What Continuum reads today |
| --- | --- |
| `Something.deltaskin` | Unzips, reads `info.json`, maps buttons, loads PDF/PNG art, applies `screens` |
| `Something.zip` with the same contents | Same as `.deltaskin` |
| Bare `info.json` | Layout + screens only (no ZIP assets to draw) |

Import applies hitbox positions from `items`, draws `assets` PDF/PNG behind the pad, and
places the game picture from the first `screens[].outputFrame` when present. Per-button
thumbstick artwork and press animations remain stubbed.

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

| System | `gameTypeIdentifier` |
| --- | --- |
| Game Boy / Color | `com.rileytestut.delta.game.gbc` |
| Game Boy Advance | `com.rileytestut.delta.game.gba` |
| Nintendo DS | `com.rileytestut.delta.game.ds` |
| NES | `com.rileytestut.delta.game.nes` |
| SNES | `com.rileytestut.delta.game.snes` |
| Nintendo 64 | `com.rileytestut.delta.game.n64` |
| Sega Genesis | `com.rileytestut.delta.game.genesis` |

PlayStation is Continuum-only here (Delta does not ship a PS1 `gameTypeIdentifier`). A
PS1-oriented pack without a recognised id still imports frames onto the editor's current
preview console.

Unknown identifiers still import button frames; only the layout-editor preview console
hint is skipped.

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

- Simple button: `"inputs": ["a"]` (or several names for a combo; Continuum takes the first
  mappable name).
- D-pad / stick: `"inputs": { "up": "up", "down": "down", "left": "left", "right": "right" }`
  — Continuum places the D-pad cluster from that item's frame centre.
- Names Continuum maps today: `a`, `b`, `x`, `y`, `l`, `r`, `l2`, `r2`, `select`, `start`,
  and the four D-pad directions. Delta-only names (`menu`, `quickSave`, `z`, C-buttons, …)
  are skipped with a note in the import summary.

## Sample

A minimal GBA portrait `info.json` lives at
[`docs/samples/delta-skin-gba-info.json`](samples/delta-skin-gba-info.json). A packed
[`docs/samples/continuum-sample-gba.deltaskin`](samples/continuum-sample-gba.deltaskin)
includes a tiny PNG so Import can exercise art + `screens` together. You can also import
the bare `info.json` to exercise layout/screens without ZIP assets.

## What still does not work

- Landscape / iPad / splitView selection beyond the preference order above
- Thumbstick artwork, press animations, CoreImage `filters`, or extension fields beyond
  Delta's documented `info.json`
- Multi-screen DS layouts beyond using the first `screens[]` entry for the picture hole
- Device proof of the import path (code only until a build is tried on a phone)

Use **Import .deltaskin** in the on-screen control layout editor. Cancelling the picker or
picking nothing leaves a clear error on that panel; a bad package names what failed.
