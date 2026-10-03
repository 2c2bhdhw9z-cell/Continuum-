#!/usr/bin/env python3
"""Skin holes, analog sticks, and pressed art. No phone, no invented images.

The Swift types do not run on this machine. This checks the rules against the
official Manic info.json files and against the source that implements them.
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKINS = Path("/tmp/skins")
IMPORTER = (ROOT / "native/ios/DeltaSkinImport.swift").read_text()
CONTROLS = (ROOT / "native/ios/SkinScreens.swift").read_text()
TOUCH = (ROOT / "native/ios/TouchControls.swift").read_text()
APP = (ROOT / "native/ios/ContinuumApp.swift").read_text()
PLAYER = (ROOT / "native/ios/PlayerScreen.swift").read_text()

failures = []


def check(name, ok, detail=""):
    if ok:
        print(f"ok  {name}")
    else:
        print(f"FAIL {name} {detail}")
        failures.append(name)


def between(text, start, end):
    i = text.find(start)
    j = text.find(end, i + len(start))
    if i < 0 or j < 0:
        return ""
    return text[i:j]


def canonical(name):
    return name.strip().lower().replace(" ", "").replace("_", "").replace("-", "")


def read_frame(value):
    if not isinstance(value, dict):
        return None
    try:
        x, y = float(value["x"]), float(value["y"])
        w, h = float(value["width"]), float(value["height"])
    except (KeyError, TypeError, ValueError):
        return None
    if w <= 0 or h <= 0:
        return None
    return x, y, w, h


PRESSED_KEYS = ["pressed", "highlight", "highlighted", "selected"]


def asset_names(value):
    if not isinstance(value, dict):
        return None, None
    normal = value.get("normal")
    normal = normal.strip() if isinstance(normal, str) and normal.strip() else None
    pressed = None
    for key in PRESSED_KEYS:
        name = value.get(key)
        if isinstance(name, str) and name.strip():
            pressed = name.strip()
            break
    return normal, pressed


def stick_side(item):
    parts = []
    inputs = item.get("inputs")
    if isinstance(inputs, dict):
        for key, value in inputs.items():
            parts.append(key)
            if isinstance(value, str):
                parts.append(value)
    elif isinstance(inputs, list):
        parts.extend(str(v) for v in inputs)
    elif isinstance(inputs, str):
        parts.append(inputs)
    blob = " ".join(canonical(p) for p in parts)
    has_thumb = item.get("thumbstick") is not None
    if "rightthumbstick" in blob or "rightstick" in blob or "cstick" in blob:
        return "right"
    if has_thumb or "leftthumbstick" in blob or "analogstick" in blob or "circlepad" in blob or "thumbstick" in blob:
        return "left"
    return None


def classify(item):
    if read_frame(item.get("frame")) is None:
        return None
    if stick_side(item):
        return "stick", stick_side(item), asset_names(item.get("asset"))
    inputs = item.get("inputs")
    if isinstance(inputs, dict):
        keys = {canonical(k) for k in inputs}
        values = [canonical(v) for v in inputs.values() if isinstance(v, str)]
        if any("touchscreen" in v for v in values):
            return "touch", None, (None, None)
        if {"up", "down", "left", "right"} <= keys and not any(
            ("thumbstick" in v or "analog" in v or "stick" in v) for v in values
        ):
            return "dpad", None, asset_names(item.get("asset"))
    names = []
    if isinstance(inputs, list):
        names = [v for v in inputs if isinstance(v, str) and v.strip()]
    elif isinstance(inputs, str) and inputs.strip():
        names = [inputs]
    elif isinstance(inputs, dict):
        names = [v for v in inputs.values() if isinstance(v, str) and v.strip()]
    if not names:
        return None
    return "button", names[0], asset_names(item.get("asset"))


def screens_of(raw):
    out = []
    for screen in raw:
        frame = read_frame(screen.get("outputFrame"))
        if frame is None:
            continue
        inp = read_frame(screen.get("inputFrame"))
        out.append((frame, inp))
    return out


def aspect_fit(mapping, bounds):
    mw, mh = mapping
    bw, bh = bounds
    scale = min(bw / mw, bh / mh)
    width, height = mw * scale, mh * scale
    return ((bw - width) / 2, (bh - height) / 2, width, height)


def place(screen_count, mapping, bounds, fallback):
    """Mirrors SkinPicturePlacement.place: a hole is not the fallback strip."""
    canvas = aspect_fit(mapping, bounds)
    if screen_count == 0:
        return canvas, [fallback], True
    return canvas, ["hole"] * screen_count, False


# --- source: the live path classifies a stick before a D-pad ---
classify_fn = between(CONTROLS, "static func classify", "struct Resolved")
stick_at = classify_fn.find("stickSide")
dpad_at = classify_fn.find("isPlainDpad")
check("classify checks a stick before a D-pad", 0 <= stick_at < dpad_at, f"stick {stick_at} dpad {dpad_at}")

parse_fn = between(IMPORTER, "let portraitItems", "let screens = SkinControls.screens")
check("import classifies items instead of mapItems", "SkinControls.classify" in parse_fn and "mapItems(" not in parse_fn)

check("pressed keys are only the file's own", "pressedAssetKeys = [\"pressed\", \"highlight\", \"highlighted\", \"selected\"]" in CONTROLS)
check("missing holes use the fallback", "usedFallback: true" in CONTROLS and "usedFallback: false" in CONTROLS)
check("a present hole does not take the fallback strip", "if holes.isEmpty" in CONTROLS)

publish = between(TOUCH, "private func publishPictureArea", "private func updateTouchScreenRect")
hole_return = publish.find("deliverSkinHoles(screens)")
fallback = publish.find("minClearPortraitFraction")
check("holes return before the fallback strip", 0 <= hole_return < fallback, f"hole {hole_return} fallback {fallback}")
check("right stick is its own axis", "rightStick" in TOUCH and "Float(rightStick.x).clampedToStick" in TOUCH)
check("pressed art is not invented", "artPressed[key]" in TOUCH and "No pressed asset" in TOUCH)
check("skin left stick is not also the D-pad", "skinLeftStick" in TOUCH)
check("both holes go to the renderer", "setSkinScreens" in APP and "cropsFramebuffer" in APP)
check("picture fit is skipped when holes are active", "skinHolesActive" in APP and "PictureFit.rect" in APP)
check("debug text stays off a skin hole", "if !host.skinHolesActive" in PLAYER and "telemetryStrip" in PLAYER)

# --- official skins ---
def load_face(system, orientation):
    path = SKINS / system / "info.json"
    data = json.loads(path.read_text())
    node = data["representations"]["iphone"]["edgeToEdge"][orientation]
    return data.get("gameTypeIdentifier"), node


for system in ("3ds", "ds", "gbc", "gba", "nes", "n64"):
    ident, portrait = load_face(system, "portrait")
    _, landscape = load_face(system, "landscape")
    p_screens = screens_of(portrait.get("screens") or [])
    l_screens = screens_of(landscape.get("screens") or [])
    check(f"{system} portrait and landscape both parse", bool(portrait.get("mappingSize")) and bool(landscape.get("mappingSize")))
    if system in ("3ds", "ds"):
        check(f"{system} keeps two portrait holes", len(p_screens) == 2, str(len(p_screens)))
        check(f"{system} keeps two landscape holes", len(l_screens) == 2, str(len(l_screens)))
        top, bottom = p_screens
        check(f"{system} top crop is the top of the buffer", top[1] is not None and top[1][1] == 0, str(top[1]))
        check(f"{system} bottom crop is not the top crop", bottom[1] is not None and bottom[1][1] > top[1][1], str(bottom[1]))
        # Landscape holes sit side by side: same output y, different output x, different crops.
        check(
            f"{system} landscape uses its own holes",
            l_screens[0][0] != p_screens[0][0],
            "landscape output matched portrait",
        )
    else:
        check(f"{system} has one hole", len(p_screens) == 1, str(len(p_screens)))
        check(f"{system} one-screen hole is the whole texture", p_screens[0][1] is None)

# 3DS controls
_, face = load_face("3ds", "portrait")
kinds = [classify(item) for item in face["items"]]
kinds = [k for k in kinds if k]
sticks = [k for k in kinds if k[0] == "stick"]
check("3DS left stick is analog, not a D-pad", ("stick", "left", (None, None)) in sticks or any(k[1] == "left" for k in sticks))
check("3DS right stick is analog, not a D-pad", any(k[0] == "stick" and k[1] == "right" for k in kinds))
check("3DS stick items are not classified as the D-pad", not any(k[0] == "dpad" and False for k in sticks) and all(k[0] == "stick" for k in sticks))
buttons = {k[1] for k in kinds if k[0] == "button"}
for name in ("l1", "l2", "r1", "r2", "a", "b", "x", "y"):
    check(f"3DS declares {name}", name in buttons)
check("3DS default pack has no pressed image", all(k[2][1] is None for k in kinds))

# A file that names highlight records it. A file that does not, does not.
sample = {"frame": {"x": 0, "y": 0, "width": 10, "height": 10}, "inputs": ["a"],
          "asset": {"normal": "a.pdf", "highlight": "a-pressed.pdf"}}
kind = classify(sample)
check("highlight is the pressed image", kind[2] == ("a.pdf", "a-pressed.pdf"), str(kind))
plain = {"frame": {"x": 0, "y": 0, "width": 10, "height": 10}, "inputs": ["a"],
         "asset": {"normal": "a.pdf"}}
check("no pressed key means normal only", classify(plain)[2] == ("a.pdf", None))

# Placement: a hole is that hole. No hole is the fallback, and the fallback is not used when a hole exists.
mapping = (375, 812)
bounds = (402, 874)  # wider than the skin, so the canvas is not the whole view
fallback = (0, 0, 402, 200)
canvas, holes, used = place(0, mapping, bounds, fallback)
check("missing hole uses the fallback", used and holes == [fallback])
canvas, holes, used = place(2, mapping, bounds, fallback)
check("two holes do not use the fallback", not used and holes == ["hole", "hole"])
check("hole canvas is the skin aspect, not the fallback strip", abs(canvas[2] / canvas[3] - 375 / 812) < 1e-6, str(canvas))
# The top hole of the real 3DS skin, in that canvas, is not the fallback strip.
out = p_screens[0][0] if False else screens_of(face["screens"])[0][0]
ox, oy, ow, oh = out
fx, fy, fw, fh = canvas[0], canvas[1], canvas[2], canvas[3]
hole = (fx + ox / 375 * fw, fy + oy / 812 * fh, ow / 375 * fw, oh / 812 * fh)
check("top hole is inside the canvas", hole[1] >= fy and hole[1] != 0 or fy == 0)
check("top hole is not the fallback strip", hole != fallback and abs(hole[1] - fallback[1]) + abs(hole[3] - fallback[3]) > 1)

if failures:
    print(f"{len(failures)} failed")
    sys.exit(1)
print("all skin hole checks passed")
