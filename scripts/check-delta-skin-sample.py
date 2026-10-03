#!/usr/bin/env python3
"""Host check: sample Delta skin maps screens[] and assets the way Continuum expects."""
from __future__ import annotations

import json
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SAMPLE_JSON = ROOT / "docs/samples/delta-skin-gba-info.json"
SAMPLE_SKIN = ROOT / "docs/samples/continuum-sample-gba.deltaskin"


def fail(msg: str) -> None:
    print(f"FAIL: {msg}", file=sys.stderr)
    sys.exit(1)


def normalize(frame: dict, mapping: dict) -> tuple[float, float, float, float]:
    mw, mh = float(mapping["width"]), float(mapping["height"])
    if mw <= 0 or mh <= 0:
        fail("mappingSize must be positive")
    return (
        float(frame["x"]) / mw,
        float(frame["y"]) / mh,
        float(frame["width"]) / mw,
        float(frame["height"]) / mh,
    )


def main() -> None:
    data = json.loads(SAMPLE_JSON.read_text())
    portrait = data["representations"]["iphone"]["edgeToEdge"]["portrait"]
    mapping = portrait["mappingSize"]
    screens = portrait.get("screens") or []
    if not screens:
        fail("sample must include screens[] for Continuum picture placement")
    out = screens[0].get("outputFrame")
    if not out:
        fail("sample screens[0] needs outputFrame")
    x, y, w, h = normalize(out, mapping)
    # Known numbers from the committed sample: 47,120,320,213 on 414x896
    expect = (47 / 414, 120 / 896, 320 / 414, 213 / 896)
    for got, want, name in zip((x, y, w, h), expect, "xywh"):
        if abs(got - want) > 1e-9:
            fail(f"screen {name}={got} expected {want}")
    assets = portrait.get("assets") or {}
    names = [assets[k] for k in ("resizable", "medium", "large", "small") if k in assets]
    if not names:
        fail("sample needs at least one assets entry")

    if not SAMPLE_SKIN.is_file():
        fail(f"missing {SAMPLE_SKIN}")
    with zipfile.ZipFile(SAMPLE_SKIN) as zf:
        names_in = set(zf.namelist())
        if "info.json" not in names_in:
            fail("deltaskin missing flat info.json")
        info = json.loads(zf.read("info.json"))
        art_name = names[0]
        if art_name not in names_in:
            fail(f"deltaskin missing asset {art_name}")
        art = zf.read(art_name)
        if len(art) < 8 or art[:8] != b"\x89PNG\r\n\x1a\n":
            # tolerate PDF later; sample is PNG
            if not art_name.lower().endswith(".pdf"):
                fail(f"asset {art_name} is not a PNG")
        items = (
            info["representations"]["iphone"]["edgeToEdge"]["portrait"]["items"]
        )
        if len(items) < 3:
            fail("sample needs several mappable items")

    edge = data["representations"]["iphone"]["edgeToEdge"]
    if "landscape" not in edge:
        fail("sample must keep a landscape representation, not only portrait")
    land = edge["landscape"]
    lm = land["mappingSize"]
    lout = land["screens"][0]["outputFrame"]
    lx, ly, lw, lh = normalize(lout, lm)
    # 200,40,480,320 on 896x414
    lexpect = (200 / 896, 40 / 414, 480 / 896, 320 / 414)
    for got, want, name in zip((lx, ly, lw, lh), lexpect, "xywh"):
        if abs(got - want) > 1e-9:
            fail(f"landscape screen {name}={got} expected {want}")
    # Portrait hole stretched onto a wide phone changes aspect. Aspect-fit must not.
    pw, ph = 896.0, 414.0
    mw, mh = 414.0, 896.0
    stretched_aspect = (320 / 414 * pw) / (213 / 896 * ph)
    scale = min(pw / mw, ph / mh)
    fitted_w = (320 / mw) * (mw * scale)
    fitted_h = (213 / mh) * (mh * scale)
    fitted_aspect = fitted_w / fitted_h
    hole_aspect = 320 / 213
    if abs(fitted_aspect - hole_aspect) > 1e-6:
        fail(f"aspect-fit changed the hole ({fitted_aspect} vs {hole_aspect})")
    if abs(stretched_aspect - hole_aspect) < 0.2:
        fail("stretch fixture no longer demonstrates the bug")
    if fitted_w >= pw * 0.9:
        fail("aspect-fit canvas is still full width")

    importer = (ROOT / "native/ios/DeltaSkinImport.swift").read_text()
    if "bestLandscapeRank" not in importer or "pickOrientations" not in importer:
        fail("importer does not keep a landscape representation")
    if 'orientation == "portrait"' in importer and "return chosen" in importer:
        # The old bug returned as soon as iphone/edgeToEdge/portrait was found.
        if "return chosen" in importer.split("func pickOrientations", 1)[-1].split("func buildFace", 1)[0]:
            fail("pickOrientations still returns on the first portrait hit")
    controls = (ROOT / "native/ios/TouchControls.swift").read_text()
    if "aspectFitCanvas" not in controls and "aspectFitCanvas" not in importer:
        fail("no aspect-fit canvas for the skin hole")
    if "drawing full width instead" in controls and "skinCanvas" not in controls:
        fail("landscape give-up still has no skin canvas")

    print(
        "OK: sample GBA skin screens normalize to "
        f"({x:.4f},{y:.4f},{w:.4f},{h:.4f}); landscape "
        f"({lx:.4f},{ly:.4f},{lw:.4f},{lh:.4f}); "
        f"aspect-fit hole {fitted_aspect:.3f} not stretched {stretched_aspect:.3f}; "
        f"package has info.json + {names[0]}"
    )


if __name__ == "__main__":
    main()
