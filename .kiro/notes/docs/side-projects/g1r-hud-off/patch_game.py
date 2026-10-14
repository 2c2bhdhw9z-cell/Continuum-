#!/usr/bin/env python3
"""Make a copy of the gen1recomp++ iPhone app that hides Apple's stats box itself.

usage: patch_game.py original.ipa AppleHudOff.lua output.ipa

Changes, and nothing else (inside game.love, the game's own code):
  * adds src/core/AppleHudOff.lua (see that file)
  * main.lua: one line near the top that loads it
The app's program files, Info.plist and every launcher file are untouched.
"""
import io
import os
import plistlib
import sys
import zipfile

ANCHOR = 'local SwitchDiagnostics = require("src.debug.SwitchDiagnostics")\n'
INSERT = ('-- Personal patch: hide Apple\'s Metal Performance HUD on iPhone (src/core/AppleHudOff.lua).\n'
          'pcall(require, "src.core.AppleHudOff")\n\n')
NEW_FILE = "src/core/AppleHudOff.lua"


def patch_love(love_bytes, module_bytes):
    zin = zipfile.ZipFile(io.BytesIO(love_bytes))
    names = zin.namelist()
    if NEW_FILE in names:
        raise SystemExit("game.love already patched")
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w") as zout:
        for zi in zin.infolist():
            data = zin.read(zi.filename)
            if zi.filename == "main.lua":
                text = data.decode("utf-8")
                if text.count(ANCHOR) != 1:
                    raise SystemExit("main.lua anchor line not found exactly once")
                data = text.replace(ANCHOR, INSERT + ANCHOR).encode("utf-8")
            zout.writestr(zi, data, compress_type=zi.compress_type)
            if zi.filename == "main.lua":
                nz = zipfile.ZipInfo(NEW_FILE, date_time=zi.date_time)
                nz.external_attr = zi.external_attr
                nz.create_system = zi.create_system
                zout.writestr(nz, module_bytes, compress_type=zipfile.ZIP_DEFLATED)
    return out.getvalue()


def main():
    src, module_path, dst = sys.argv[1:4]
    with open(module_path, "rb") as f:
        module_bytes = f.read()
    zin = zipfile.ZipFile(src)
    app_dir = next(n for n in zin.namelist()
                   if n.startswith("Payload/") and n.endswith(".app/") and n.count("/") == 2)
    plistlib.loads(zin.read(app_dir + "Info.plist"))  # sanity: it's an app
    love_name = app_dir + "game.love"
    new_love = patch_love(zin.read(love_name), module_bytes)

    if os.path.exists(dst):
        os.remove(dst)
    with zipfile.ZipFile(dst, "w") as zout:
        for zi in zin.infolist():
            data = new_love if zi.filename == love_name else zin.read(zi.filename)
            zout.writestr(zi, data, compress_type=zipfile.ZIP_STORED if zi.is_dir() else zipfile.ZIP_DEFLATED)
    print(f"wrote {dst}")


if __name__ == "__main__":
    main()
