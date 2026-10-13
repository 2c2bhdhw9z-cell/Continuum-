#!/usr/bin/env bash
# Downloads the Z80 test suites into test-suites/ (git-ignored: none of it is committed), each
# checked against a pinned sha256 before it is kept:
#   - SingleStepTests' Z80 suite (MIT): one JSON file per opcode, 1000 cases each, 1604 files.
#   - ZEXDOC and ZEXALL, Frank D. Cringle's Z80 instruction exercisers (GPL-2.0-or-later), from
#     YAZE-AG 2.51.3 as kept in agn453/ZEXALL.
# Without them tests/z80_singlestep.rs and tests/z80_zex.rs report SKIPPED and pass.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p test-suites && cd test-suites

if command -v sha256sum >/dev/null 2>&1; then SHA256="sha256sum"; else SHA256="shasum -a 256"; fi
sum() { $SHA256 "$1" | cut -d' ' -f1; }

# A file at a URL, kept only if its sha256 matches; one already here and matching is not fetched.
fetch() {
  local url="$1" out="$2" want="$3" got
  if [ -s "$out" ] && [ "$(sum "$out")" = "$want" ]; then return 0; fi
  curl -fsSL "$url" -o "$out.part"
  got=$(sum "$out.part")
  if [ "$got" != "$want" ]; then
    rm -f "$out.part"
    echo "$out: sha256 $got, expected $want" >&2
    return 1
  fi
  mv "$out.part" "$out"
}

Z=https://raw.githubusercontent.com/agn453/ZEXALL/8f71d418bae69a476a5a0e5c6e122c8801b8d9f4
fetch $Z/zexdoc.com zexdoc.com 34923a7ed82285d3038b2d54bd64899e12173eebb61f9d07b4fc72e78af2ae8f
fetch $Z/zexall.com zexall.com 6e2da55147a04f28d303d5da6a1e6b771557ac244653590a0f24a2d39c8537e8
fetch $Z/LICENSE zexall-LICENSE 8177f97513213526df2cf6184d8ff986c675afb514d4e68a404010521b880643

# SingleStepTests/z80 at one commit, as GitHub's tarball of it (plain HTTPS, which works where git
# can't reach GitHub). Only v1/ and the licence are kept. The check is one sha256 over the sorted
# list of every JSON file's sha256, so it holds however GitHub packs the tarball.
SST_COMMIT=ebe1875d48f374bcfd4b505d8eb8ee751568b5f7
SST_SUM=05d8e1eb73e5876bb78903000fae081b6117ddd0cccbdf423aee7760a9ebe4ea
sst_sum() { (cd "$1" && find v1 -type f -name '*.json' -print0 | LC_ALL=C sort -z | xargs -0 $SHA256 | $SHA256 | cut -d' ' -f1); }
if [ ! -d singlestep-z80 ] || [ "$(sst_sum singlestep-z80)" != "$SST_SUM" ]; then
  rm -rf singlestep-z80 singlestep-z80.part
  mkdir singlestep-z80.part
  curl -fsSL "https://codeload.github.com/SingleStepTests/z80/tar.gz/$SST_COMMIT" \
    | tar -xz -C singlestep-z80.part --strip-components=1
  # Keep the tests and their licence; the generator and the rest of the repository go.
  find singlestep-z80.part -mindepth 1 -maxdepth 1 ! -name v1 ! -name LICENSE -exec rm -rf {} +
  got=$(sst_sum singlestep-z80.part)
  if [ "$got" != "$SST_SUM" ]; then
    rm -rf singlestep-z80.part
    echo "singlestep-z80: sha256 $got, expected $SST_SUM" >&2
    exit 1
  fi
  mv singlestep-z80.part singlestep-z80
fi
ls
