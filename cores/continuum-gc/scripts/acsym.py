#!/usr/bin/env python3
# Maps Animal Crossing (GAFE01) addresses to function names, for reading the GameCube core's
# "HOT" read-out and traces. The symbol list is ac-decomp's config/GAFE01_00/symbols.txt
# (https://github.com/ACreTeam/ac-decomp); read for names only, never copied into the core.
# Usage: AC_SYMBOLS=path/to/symbols.txt acsym.py 8007E100 [more addresses...]
import os, re, sys
path = os.environ.get('AC_SYMBOLS', '/projects/sandbox/.scratch/ac_symbols.txt')
syms = []
for line in open(path):
    m = re.match(r'(\S+) = \.text:0x([0-9A-Fa-f]+); // type:function size:0x([0-9A-Fa-f]+)', line)
    if m:
        syms.append((int(m.group(2), 16), int(m.group(3), 16), m.group(1)))
syms.sort()
for a in sys.argv[1:]:
    x = int(a, 16)
    hits = [s for s in syms if s[0] <= x + 0xFF and x < s[0] + s[1]]
    print(a, ', '.join(f"{n} ({s:08X}+{sz:X})" for s, sz, n in hits) or '(not in the static program: a loaded module)')
