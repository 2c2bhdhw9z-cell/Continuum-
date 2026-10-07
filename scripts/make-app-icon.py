#!/usr/bin/env python3
"""Build Continuum's app icon set, from a supplied picture or from the drawing below.

STATE: THE APP SHIPS NO ICON. The drawn triangle in this file was shown to the owner as
one of four options and rejected along with the other three, so it was taken back out of
the app in build 135 and the catalogue it wrote is not in the repository. Nothing here
runs by default; see USAGE at the bottom.

  python3 scripts/make-app-icon.py --from <picture>   fit a supplied picture to all sizes
  python3 scripts/make-app-icon.py --drawn            use the triangle drawn in this file

Needs: pip install Pillow

THE RULE THIS FILE EXISTS UNDER
-------------------------------
The owner sees and approves artwork BEFORE it goes near the app. That was got wrong once:
the drawn icon was committed and a build shipped it before they had looked at it, which is
exactly what they had asked not to happen. Hence no default mode, and hence the project.yml
wiring being absent rather than dormant: putting an icon in the app is now a deliberate act
that cannot happen as a side effect of running this script.

--from accepts any format and any size, reads the real format from the file's BYTES rather
than its name, flattens transparency onto black (iOS refuses an icon with an alpha channel,
and that is the usual reason a hand-made icon silently never shows up), and centre-crops to
square. So "send a picture and I will fit it" is one command.

THE DRAWN DESIGN, AND WHY
-------------------------
The colours are not invented. They are read straight out of the app:

  accent red    #ED2A4A   Play buttons, the active tab, the logo tile
  metadata teal #5CDBC9   secondary metadata
  pure black    #000000   the shell behind everything

So the icon is a play triangle, rounded, filled with a red-to-teal gradient, sitting on
pure black with a soft red bloom under it. One shape, because an app icon is read at
40 pixels on a home screen far more often than at 1024 in a store listing, and anything
more detailed than one shape turns to mud at that size.

The triangle's corners are rounded by taking the Minkowski sum of a smaller triangle with
a disc: inset each edge by the corner radius, then stroke that inset outline with a
round-jointed pen of twice that radius. The straight edges land back exactly where the
sharp triangle's edges were, and the corners come out as true circular arcs.

APPLE'S RULES THIS OBEYS
------------------------
- No alpha channel. iOS rejects, or renders black-boxed, an icon with transparency, so
  every file is flattened onto the black background and saved as RGB.
- No rounded corners of our own, and no drop shadow. iOS applies the squircle mask itself;
  pre-rounding it produces a visible double corner.
- The art stays inside the middle ~80%, so the mask never clips the triangle.
"""

import json
import math
import os
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter

# ---------------------------------------------------------------------------
# The palette, lifted from the app
# ---------------------------------------------------------------------------

ACCENT_RED = (0xED, 0x2A, 0x4A)   # rgb(0.93, 0.16, 0.29) in the Swift
METADATA_TEAL = (0x5C, 0xDB, 0xC9)  # rgb(0.36, 0.86, 0.79)
BLACK = (0x00, 0x00, 0x00)

# The triangle is shaded in RED ONLY, deep at its base to bright at its point.
#
# The first attempt ramped the body from the accent red straight to the metadata teal, and
# it was wrong in a way worth recording: those two colours sit on opposite sides of the
# colour wheel, so mixing them passes through grey. Measured across the middle of the
# shape it went #CB5468 (washed-out pink) to #979395 (grey) to #69CBBE, which at 40 pixels
# is a muddy smear rather than a red play button. Red is the app's Play colour, so the Play
# triangle is red; the teal earns its place as the cool light coming off the point, where
# it never has to average with the red.
BODY_DEEP = (0xB8, 0x16, 0x40)    # base of the triangle, bottom left
BODY_BRIGHT = (0xFF, 0x46, 0x63)  # its point, top right
# The lit edge. A highlight on a red surface is a BRIGHTER RED, not the complementary
# colour: the first attempt used the metadata teal here and the thin band measured
# #BE7986, a washed-out pink, for the same averaging reason as above. The teal therefore
# only ever appears OUTSIDE the shape, as the halo off the point, where it sits on black
# and stays teal.
RIM_HIGHLIGHT = (0xFF, 0x9E, 0xAC)
# Those two average to roughly #E63557 across the body, which is the accent red to the
# eye, so the icon still reads as exactly the colour the app's buttons use.

# ---------------------------------------------------------------------------
# Geometry, as fractions of the icon's side so every size is identical art
# ---------------------------------------------------------------------------

# These are the SHARP triangle. Rounding a corner always pulls the shape back from its
# sharp vertex, and at the point of a play triangle the angle is narrow so it pulls back a
# long way: the first attempt used a 0.050 radius and measured 394x437 on a 1024 icon
# instead of the 445x512 asked for, with a visibly blunt point. The numbers below are
# chosen so that the ROUNDED result lands at about 476x509, which the check at the bottom
# of this file verifies rather than trusts.
TRI_HEIGHT = 0.580   # vertical extent of the sharp triangle
TRI_WIDTH = 0.540    # horizontal extent
TRI_CX = 0.521       # nudged right of centre: a right-pointing triangle carries its
TRI_CY = 0.500       # mass on the left, so dead-centring it looks left-heavy
CORNER_R = 0.032     # corner radius: soft, but the point survives it

# The blooms. Two numbers matter here and both were got wrong once:
#   Strength. At 0.42 / 0.26 the glow measured #22090D just outside the shape, which is
#   black to the eye: all cost, no effect.
#   Reach. At 0.80 / 0.60 with a 0.065 blur it went the other way, and the two colours
#   overlapped into a greenish-white haze hanging around the whole triangle rather than a
#   bloom coming off it.
# So: tighter blur, moderate strength, and the teal CONFINED to the right of the point
# (TEAL_GATE) so it can never wash over the top or the base.
GLOW_BLUR = 0.042    # red bloom softness; smaller means it hugs the shape
GLOW_DROP = 0.018    # how far the red bloom sits below the triangle
GLOW_RED = 0.50      # bloom strength, red, behind the whole shape
# The teal is a WIDE, WEAK wash rather than a tight glow. Tight and strong (0.45 at the
# same 0.042 blur) read as a smudge of dirt sitting beside the point; spread over three
# times the distance at half the strength it reads as cool light in the room instead.
TEAL_BLUR = 0.130    # teal wash softness
GLOW_SHIFT = 0.085   # how far the teal wash sits to the RIGHT
GLOW_TEAL = 0.22     # its strength: the app's metadata teal, kept outside the shape so it
                     # never has to average with the red
TEAL_REACH = 0.300   # how far the teal wash reaches from the point before it is gone

SUPERSAMPLE = 4096   # drawn once this big, then reduced to each real size

# The classic iPhone + iPad + marketing set. Deliberately NOT the newer single 1024
# "universal" entry: every Xcode understands this form, and the runner's Xcode version is
# not pinned by this repo.
ICON_SET = [
    ("iphone", "20x20", "2x", 40),
    ("iphone", "20x20", "3x", 60),
    ("iphone", "29x29", "2x", 58),
    ("iphone", "29x29", "3x", 87),
    ("iphone", "40x40", "2x", 80),
    ("iphone", "40x40", "3x", 120),
    ("iphone", "60x60", "2x", 120),
    ("iphone", "60x60", "3x", 180),
    ("ipad", "20x20", "1x", 20),
    ("ipad", "20x20", "2x", 40),
    ("ipad", "29x29", "1x", 29),
    ("ipad", "29x29", "2x", 58),
    ("ipad", "40x40", "1x", 40),
    ("ipad", "40x40", "2x", 80),
    ("ipad", "76x76", "1x", 76),
    ("ipad", "76x76", "2x", 152),
    ("ipad", "83.5x83.5", "2x", 167),
    ("ios-marketing", "1024x1024", "1x", 1024),
]


def triangle_points(side):
    """The three corners of the play triangle, in pixels, point to the right."""
    hv = TRI_HEIGHT * side
    wv = TRI_WIDTH * side
    cx = TRI_CX * side
    cy = TRI_CY * side
    return [
        (cx - wv / 2.0, cy - hv / 2.0),  # top left
        (cx + wv / 2.0, cy),             # the point
        (cx - wv / 2.0, cy + hv / 2.0),  # bottom left
    ]


def inset_polygon(points, r):
    """Move every edge of a convex polygon `r` pixels inward.

    Each corner slides along its own angle bisector by r / sin(theta/2), which is the
    distance at which both of its edges have moved in by exactly r.
    """
    n = len(points)
    out = []
    for i in range(n):
        prev_pt = points[(i - 1) % n]
        cur = points[i]
        next_pt = points[(i + 1) % n]

        # Unit vectors along the two edges, both pointing AWAY from this corner.
        e1 = (prev_pt[0] - cur[0], prev_pt[1] - cur[1])
        e2 = (next_pt[0] - cur[0], next_pt[1] - cur[1])
        l1 = math.hypot(*e1) or 1.0
        l2 = math.hypot(*e2) or 1.0
        e1 = (e1[0] / l1, e1[1] / l1)
        e2 = (e2[0] / l2, e2[1] / l2)

        # The bisector points into the polygon because the edges point outward from here.
        bx, by = e1[0] + e2[0], e1[1] + e2[1]
        bl = math.hypot(bx, by) or 1.0
        bx, by = bx / bl, by / bl

        # Half the interior angle, from the dot product of the two edge directions.
        cos_theta = max(-1.0, min(1.0, e1[0] * e2[0] + e1[1] * e2[1]))
        half = math.acos(cos_theta) / 2.0
        step = r / max(math.sin(half), 1e-6)

        out.append((cur[0] + bx * step, cur[1] + by * step))
    return out


def rounded_triangle_mask(side):
    """An 8-bit mask of the rounded play triangle, anti-aliased by supersampling.

    The shape is the inset triangle grown back out by a disc of the corner radius, drawn
    in three explicit pieces: the inset body, a filled circle at each inset corner, and
    each inset edge stroked at twice the radius.

    It is spelled out like that on purpose. The obvious one-liner is to stroke the closed
    outline with `joint="curve"`, but Pillow only rounds a polyline's INTERIOR joints: the
    place where the line starts and ends keeps its flat caps, and since that point is the
    triangle's top-left corner, the first version of this icon had a visible square step
    bitten out of exactly that corner. Circles at the corners cannot have that bug.
    """
    r = CORNER_R * side
    inner = inset_polygon(triangle_points(side), r)

    mask = Image.new("L", (side, side), 0)
    d = ImageDraw.Draw(mask)
    d.polygon(inner, fill=255)
    for (x, y) in inner:
        d.ellipse((x - r, y - r, x + r, y + r), fill=255)
    for i in range(len(inner)):
        d.line([inner[i], inner[(i + 1) % len(inner)]], fill=255, width=int(round(2 * r)))
    return mask


def body_shading(side):
    """The red ramp that fills the triangle: deep at the base, bright at the point.

    Built one row at a time from a 256-step ramp rather than per pixel, because the
    shading only varies along one axis and the supersampled canvas is 16 million pixels.
    """
    # Up and to the right, so the brightest part of the shape is the point, under the
    # cool rim light added in render().
    axis = (1.0 / math.sqrt(2), -1.0 / math.sqrt(2))
    projections = [p[0] * axis[0] + p[1] * axis[1] for p in triangle_points(side)]
    lo, hi = min(projections), max(projections)
    span = (hi - lo) or 1.0

    img = Image.new("RGB", (side, side), BODY_DEEP)
    d = ImageDraw.Draw(img)
    # Lines of constant brightness run perpendicular to the axis, so at 45 degrees. Walk
    # the diagonal and draw each as a thick line; one per pixel of travel is plenty.
    steps = int(span * math.sqrt(2)) + side
    for i in range(steps + 1):
        t = i / steps
        # Position along the diagonal in canvas coordinates. The band starts beyond the
        # bottom-left of the shape and ends beyond its point, so the shape is covered.
        p = lo + t * span
        cx, cy = p * axis[0], p * axis[1]
        tt = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
        colour = tuple(
            int(round(BODY_DEEP[c] + (BODY_BRIGHT[c] - BODY_DEEP[c]) * tt))
            for c in range(3)
        )
        # A line perpendicular to the axis, long enough to cross the whole canvas.
        d.line(
            [(cx - side, cy - side), (cx + side, cy + side)],
            fill=colour,
            width=4,
        )
    return img


def tip_falloff(side):
    """A soft round mask centred on the triangle's point, fading out with distance.

    This confines the teal wash to the one place it belongs. A straight-edged gate was
    tried first — black left of the point, white right of it — and it left a visible
    vertical seam down the icon where the wash began, because the blurred shape is still
    strongly opaque there. A radial falloff has no straight edge anywhere, so the wash
    reads as light coming off the point and nothing else.

    Computed small and scaled up: it is a smooth blob, so the interpolation costs nothing
    in quality and saves working over sixteen million pixels.
    """
    pts = triangle_points(side)
    tip_x = max(p[0] for p in pts)
    tip_y = TRI_CY * side
    radius = TEAL_REACH * side

    small = 256
    img = Image.new("L", (small, small), 0)
    px = img.load()
    k = side / float(small)
    for y in range(small):
        dy = (y + 0.5) * k - tip_y
        for x in range(small):
            dx = (x + 0.5) * k - tip_x
            t = 1.0 - math.sqrt(dx * dx + dy * dy) / radius
            if t <= 0.0:
                continue
            # Smoothstep, so there is no edge where it reaches zero.
            px[x, y] = int(round(255 * (t * t * (3.0 - 2.0 * t))))
    return img.resize((side, side), Image.BICUBIC)


def rim_light(mask, side):
    """A thin cool edge along the triangle's upper side, fading in toward the point.

    Subtracting the shape from a copy of itself nudged downward leaves a band hugging its
    upper boundary. Weighting that band so it is strongest at the point keeps it reading
    as one light source rather than an outline.
    """
    thickness = max(1, int(round(0.013 * side)))
    lower = mask.transform(
        mask.size,
        Image.AFFINE,
        (1, 0, 0, 0, 1, -thickness),
        resample=Image.BILINEAR,
    )
    band = ImageChops.subtract(mask, lower)

    # Fade from nothing at the base to full at the point.
    pts = triangle_points(side)
    x0 = min(p[0] for p in pts)
    x1 = max(p[0] for p in pts)
    ramp = Image.new("L", (side, 1), 0)
    rp = ramp.load()
    for x in range(side):
        t = (x - x0) / max(x1 - x0, 1.0)
        t = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
        rp[x, 0] = int(round(255 * (t ** 2.2)))
    band = ImageChops.multiply(band, ramp.resize((side, side)))

    layer = Image.new("RGB", (side, side), RIM_HIGHLIGHT)
    return layer, band


def bloom(mask, side, colour, strength, blur, dx=0.0, dy=0.0):
    """One soft coloured glow derived from the shape itself, sitting under it.

    dx and dy are fractions of the side, positive meaning right and down. PIL's AFFINE
    transform maps an OUTPUT pixel back to an input one, so moving the image right by n
    means sampling from x - n, hence the negated offsets.
    """
    g = mask.filter(ImageFilter.GaussianBlur(blur * side))
    if dx or dy:
        g = g.transform(
            g.size,
            Image.AFFINE,
            (1, 0, -dx * side, 0, 1, -dy * side),
            resample=Image.BILINEAR,
        )
    g = g.point(lambda v: int(v * strength))
    layer = Image.new("RGB", (side, side), colour)
    return layer, g


def render(side):
    """The finished icon at `side` pixels, opaque RGB, no alpha."""
    mask = rounded_triangle_mask(side)

    canvas = Image.new("RGB", (side, side), BLACK)

    # Red bloom behind everything.
    layer, alpha = bloom(mask, side, ACCENT_RED, GLOW_RED, GLOW_BLUR, 0.0, GLOW_DROP)
    canvas = Image.composite(layer, canvas, alpha)

    # Then the cool halo, gated so it only exists to the right of the point.
    layer, alpha = bloom(
        mask, side, METADATA_TEAL, GLOW_TEAL, TEAL_BLUR, GLOW_SHIFT, 0.0
    )
    canvas = Image.composite(layer, canvas, ImageChops.multiply(alpha, tip_falloff(side)))

    # The shape itself, then the cool edge along its upper side.
    canvas.paste(body_shading(side), (0, 0), mask)

    layer, band = rim_light(mask, side)
    canvas = Image.composite(layer, canvas, band)

    return canvas


def check(side=1024):
    """Measure what was actually drawn, instead of trusting the constants.

    Rounding moves every corner, so the only honest way to know how big the triangle ended
    up and how much clearance it has from iOS's squircle mask is to measure the mask.
    """
    mask = rounded_triangle_mask(side)
    box = mask.getbbox()  # (left, upper, right, lower) of everything non-zero
    left, upper, right, lower = box
    w, h = right - left, lower - upper
    margin = min(left, upper, side - right, side - lower)
    print(
        f"==> rounded shape measures {w}x{h} px on a {side} icon "
        f"({w / side:.0%} x {h / side:.0%}), smallest margin {margin} px "
        f"({margin / side:.0%})"
    )
    # iOS masks the corners with a superellipse; art inside the middle 80% is never
    # clipped. 10% clear on every side is that, with room to spare.
    assert margin >= 0.10 * side, f"only {margin}px clearance, the mask may clip the art"
    assert w > 0.40 * side and h > 0.42 * side, f"shape too small to read: {w}x{h}"
    return box


def from_picture(path):
    """Turn a picture the owner supplied into the icon master.

    Accepts whatever it is handed. The format is read from the BYTES, never from the
    filename, because a service that hands back a JPEG named .png is not a hypothetical:
    one did exactly that on 7 October and poisoned a whole chat session with the
    mismatch. Pillow reports what it really found and this prints it.

    Then: flatten any transparency onto black, because iOS refuses an icon with an alpha
    channel and that is the usual reason a hand-made icon silently never appears; and
    centre-crop to a square, because an icon slot is square and a squashed picture looks
    like a mistake.
    """
    im = Image.open(path)
    print(f"==> {os.path.basename(path)} is really {im.format} {im.size[0]}x{im.size[1]}"
          f" {im.mode}")

    if im.mode in ("RGBA", "LA", "P"):
        im = im.convert("RGBA")
        flat = Image.new("RGB", im.size, BLACK)
        flat.paste(im, (0, 0), im.split()[-1])
        im = flat
        print("    had see-through parts: flattened onto black, which iOS requires")
    else:
        im = im.convert("RGB")

    w, h = im.size
    if w != h:
        side = min(w, h)
        im = im.crop((
            (w - side) // 2, (h - side) // 2,
            (w - side) // 2 + side, (h - side) // 2 + side,
        ))
        print(f"    not square: centre-cropped to {side}x{side}")

    biggest = max(px for _, _, _, px in ICON_SET)
    if im.size[0] < biggest:
        print(f"    smaller than {biggest}: the largest slot will be scaled up and will "
              f"look a little soft")

    return im


def write_set(master):
    """Write every size plus both Contents.json files."""
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = os.path.join(here, "native", "ios", "Assets.xcassets", "AppIcon.appiconset")
    os.makedirs(out, exist_ok=True)

    assert master.mode == "RGB", "iOS will not accept an icon with an alpha channel"

    images = []
    written = {}
    for idiom, size, scale, px in ICON_SET:
        name = f"icon-{px}.png"
        if px not in written:
            master.resize((px, px), Image.LANCZOS).convert("RGB").save(
                os.path.join(out, name), "PNG", optimize=True
            )
            written[px] = name
            print(f"    {name}")
        images.append(
            {"idiom": idiom, "size": size, "scale": scale, "filename": written[px]}
        )

    with open(os.path.join(out, "Contents.json"), "w") as f:
        json.dump(
            {"images": images, "info": {"version": 1, "author": "xcode"}}, f, indent=2
        )
        f.write("\n")

    # The catalogue root. Xcode wants one even with a single set inside.
    with open(os.path.join(os.path.dirname(out), "Contents.json"), "w") as f:
        json.dump({"info": {"version": 1, "author": "xcode"}}, f, indent=2)
        f.write("\n")

    print(f"==> {len(written)} files in {out}")
    print("==> now add the catalogue back to native/ios/project.yml: a `- path: "
          "Assets.xcassets` line in the target's `sources`, and "
          "`ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon` in its `settings: base:`. "
          "Both were removed when the drawn icon was taken out.")


USAGE = """Continuum app icon builder.

  python3 scripts/make-app-icon.py --from <picture>   use a picture the owner supplied
  python3 scripts/make-app-icon.py --drawn            use the triangle drawn in this file

THERE IS NO DEFAULT ON PURPOSE. The owner rejected the drawn icon and wants to see and
approve any artwork BEFORE it goes near the app, so running this with no arguments must
not quietly put a picture in the build. --drawn still works if it is ever asked for.
"""


def main(argv):
    if len(argv) == 2 and argv[1] == "--drawn":
        check()
        print(f"==> drawing at {SUPERSAMPLE}x{SUPERSAMPLE}")
        write_set(render(SUPERSAMPLE))
        return 0

    if len(argv) == 3 and argv[1] == "--from":
        if not os.path.exists(argv[2]):
            print(f"no such file: {argv[2]}")
            return 1
        write_set(from_picture(argv[2]))
        return 0

    print(USAGE)
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
