#!/usr/bin/env bash
# Downloads the 6502 test suites into test-suites/ (git-ignored: none of it is committed), each
# checked against a pinned sha256 before it is kept:
#   - SingleStepTests' 6502 suite (MIT): one JSON file per opcode, 10,000 cases each, 256 files,
#     every bus cycle of every case listed.
#   - Klaus Dormann's 6502 functional test (GPL-3.0), the assembled image from his repository.
# Without them tests/m6502_singlestep.rs and tests/m6502_functional.rs report SKIPPED and pass.
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

K=https://raw.githubusercontent.com/Klaus2m5/6502_65C02_functional_tests/7954e2dbb49c469ea286070bf46cdd71aeb29e4b
fetch $K/bin_files/6502_functional_test.bin 6502_functional_test.bin \
  fa12bfc761e6f9057e4cc01a665a7b800ff01ae91f598af1e39a1201d01953fd

# SingleStepTests/65x02 at one commit, its 6502/v1 folder file by file (the whole repository is a
# gigabyte of other processors). The check is one sha256 over the sorted list of every file's sha256.
SST_COMMIT=2f6980a2d95757486c7bee24355c360e40e2a224
SST_SUM=3d3a8e5f571b3793b061595e307404da98d967f0f5cf5f14319506a039e3b651
sst_sum() { (cd "$1" && find v1 -type f -name '*.json' -print0 | LC_ALL=C sort -z | xargs -0 $SHA256 | $SHA256 | cut -d' ' -f1); }
if [ ! -d singlestep-6502 ] || [ "$(sst_sum singlestep-6502)" != "$SST_SUM" ]; then
  mkdir -p singlestep-6502.part/v1
  for i in $(seq 0 255); do printf '%02x\n' "$i"; done | xargs -P 6 -I{} sh -c '
    f=singlestep-6502.part/v1/{}.json
    [ -s "$f" ] || curl -fsSL --retry 3 -o "$f.tmp" \
      "https://raw.githubusercontent.com/SingleStepTests/65x02/'"$SST_COMMIT"'/6502/v1/{}.json" && { [ -s "$f" ] || mv "$f.tmp" "$f"; }'
  curl -fsSL -o singlestep-6502.part/LICENSE \
    "https://raw.githubusercontent.com/SingleStepTests/65x02/$SST_COMMIT/LICENSE"
  got=$(sst_sum singlestep-6502.part)
  if [ "$got" != "$SST_SUM" ]; then
    echo "singlestep-6502: sha256 $got, expected $SST_SUM (files kept in singlestep-6502.part)" >&2
    exit 1
  fi
  rm -rf singlestep-6502 && mv singlestep-6502.part singlestep-6502
fi
ls
