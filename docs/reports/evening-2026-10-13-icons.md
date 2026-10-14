# Icons, 13 October 2026 batch (finished 14 October)

The owner said "some of the icons needs fixing" and circled 12 in Settings, App Icon. Everything
below is **Built, untested**: no phone has shown it yet.

## The 12 circled home screen icons

| Icon | What was wrong | What changed |
|---|---|---|
| Knockout | A white rim around the black tile, with dark corners inside iPhone's rounding | Cropped just inside the rim. The black now reaches every edge |
| Clay, Glow, Ivory, Mint, Graphite, Sand | A smaller rounded tile drawn inside the picture, with a white or cream margin, a bevel and a drop shadow. On the phone it looked like an icon inside an icon | The tile's own face now reaches every edge. The C and controller are centred and fill about three quarters of the icon, like Lens, Reactor, Gem and Storm |
| Badge | The same, plus a thick silver frame | The black face now reaches the edges. The silver C and controller stay as they were, centred |
| Cloud | The same. The gold arc sat right against the tile's edge | The light blue face now reaches the edges. The C, controller, gold arc and dots are all kept, centred |
| Spectrum | The rainbow pixel ring ran off the top, left and bottom edges, and iPhone's rounded corners cut into it | The whole picture now sits at 77% size on its own black, so the full ring shows |
| Cyan | Left as it is | See below |
| Violet | Left as it is | See below |

Commits: 495733a (the nine framed icons) and d095912 (Spectrum).

**How the framed icons were fixed, without redrawing anything.** For each icon I found the inner
tile from its pixels: its edge, where the rim or bevel rolls off onto the flat face, and its
corners. The face and the artwork are kept pixel for pixel. Only the resize to 1024 changes
them. Knockout's face is pure black, so a crop was enough. For the other eight, everything
outside the face is covered by the face's own colour, carried smoothly out to the edges. That
means the rim, bevel, margin, shadow and the inner tile's corners. Clay, Cloud, Ivory, Graphite
and Sand also get the face's own grain, taken from a clean patch of the same face, so the
surface looks the same right to the edge. No new colour was added and nothing was recoloured.

**Why Cyan and Violet are unchanged.** Both pictures run past their own edges, so the missing
parts were never drawn:
- Cyan: the C is a circle; fitting its visible outline gives a 0.4 px error. It runs 27 px past
  the top and 11 px past the left. The controller's right grip runs about 22 px past the right
  edge; the controller is symmetric, so its far side should be at x 1045 of 1024. Shrinking the
  picture onto black, as planned, would show straight cut lines across the top and left of the C
  and down the controller's right side. I made a test version and they are clearly visible.
  Filling them in would be redrawing.
- Violet: the controller sits 5 px from the right edge, with its centre 71% of the way across.
  The nebula runs off the top, left and bottom. There is no frame, and the dark corners are only
  the space background. Centring it, or giving the nebula room, would mean painting nebula that
  isn't in the picture. Shrinking it shows a hard square edge where the nebula is cut.
- If the owner wants these two changed, they need new artwork, which is their call.

**Checks.**
- Each icon was measured before and after.
- The before-and-after sheet of all 12 was viewed with iOS's rounded mask. Every fixed one fills
  its tile, with no inner tile, rim, margin, shadow or corner showing.
- Edge flatness now measures 0 to 7 grey levels (out of 255) on every side of all nine framed
  icons. Before it was 16 to 255: the rim, bevel and shadow.
- Where the kept face meets the extended colour, the median step is under 1 grey level.
- All sizes were remade from the fixed 1024 picture with `scripts/make-app-icon.py`'s size list
  and Lanczos resize. Each preview in `AppIconPreviews` is a byte copy of its own `icon-180.png`.
- A re-audit of all 56 icon sets found 0 problems. It checked that every size is the same
  picture as its 1024, every preview is a byte copy, every PNG is opaque 8-bit RGB (no alpha, no
  tRNS, good CRCs), sizes match `Contents.json`, and names match across the asset folders,
  `project.yml` and `AppIcons.swift`. On a planted copy, the audit caught all six faults: a wrong
  picture, a re-encoded preview, an RGBA file, a wrong size, a missing file and a JPEG named .png.

## Icons drawn inside the app

- Checked all 69 symbol names (106 uses) in `native/ios` against Apple's availability list,
  taken from SFSafeSymbols' `name_availability.plist`. That covers ternaries, the `PillButton`
  `systemImage`, and the `sessionButton`/`holdButton` helpers.
- One name was missing on iOS 16: `square.resize` (iOS 17), on **Next resolution** in the
  in-game menu. On iOS 16 it drew nothing. It is now `arrow.up.left.and.arrow.down.right`, which
  exists since iOS 13 (5b10c45; parse-checked).
- The app asks for no custom pictures by name. The only bundle lookup is the App Icon previews,
  covered above.
- Symbols are drawn at 9 to 17 points, inside 34 to 44 point buttons. None are stretched.

## Phone checks

1. Open Settings, App Icon. Look at Knockout, Clay, Glow, Badge, Cloud, Ivory, Mint, Graphite,
   Sand and Spectrum. Each one should fill its whole tile, with no smaller square, white border
   or shadow inside it.
2. Pick Clay, tap OK when iPhone asks, then go to the home screen. The Continuum icon should be
   one clean icon, its grey reaching the rounded edge.
3. Do the same with Badge. The black should reach the edge, with no silver frame.
4. Do the same with Spectrum. The whole rainbow ring should show, with nothing cut off at the
   corners.
5. Start any game and open the in-game menu. **Next resolution** should have a small icon next
   to it.

Cyan and Violet look the same as before, on purpose (see above).

No TESTING.md line is out of date because of this. Row 28, changing the home screen icon then
restoring a backup, still applies as written.
