#!/usr/bin/env python3
# Builds a FAT32 SD card image (no partition table) holding a folder's files, for the GameCube
# core's SD adapter (test only). Usage: mkfat.py image size_mb folder
# Needs pyfatfs (pip install pyfatfs). Neverball's homebrew folder made the test card.
import os, sys
from pyfatfs.PyFat import PyFat
from pyfatfs.PyFatFS import PyFatFS
img, size_mb, src = sys.argv[1], int(sys.argv[2]), sys.argv[3]
with open(img, 'wb') as f:
    f.truncate(size_mb << 20)
pf = PyFat()
pf.mkfs(img, fat_type=32, size=size_mb << 20, label='NEVERBALL')
pf.close()
fs = PyFatFS(img)
for root, dirs, files in os.walk(src):
    rel = os.path.relpath(root, src)
    base = '/' if rel == '.' else '/' + rel.replace(os.sep, '/')
    for d in dirs:
        fs.makedir(base.rstrip('/') + '/' + d)
    for n in files:
        with open(os.path.join(root, n), 'rb') as r, fs.openbin(base.rstrip('/') + '/' + n, 'w') as w:
            w.write(r.read())
fs.close()
print('ok', img)
