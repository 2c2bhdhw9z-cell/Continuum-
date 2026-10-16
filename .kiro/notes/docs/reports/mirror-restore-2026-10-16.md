# The frozen emulator copies, put back (16 October 2026)

## What was lost

On 14 October the old releases were deleted, and `core-mirror-1` went with them. It held the frozen
copies of the 16 emulators Continuum downloads instead of building, plus `prboom.wad`. It was
re-created empty. The pinned zips themselves can't be had again: the buildbot re-zips every night.

## What a build does now (`scripts/fetch-buildbot-cores.sh`)

1. **The mirror accepts a copy by the emulator file inside it** (the dylib pin), not only by the
   zip's exact bytes. A zip is only a wrapper.
2. **New fallback: the last shipped install.** When the mirror has nothing and the buildbot's file is
   a different build, the script takes the same file out of build 185's install
   (`SHIPPED_TAG=build-185-393f12c`). It downloads the install at most once per build, and only
   when needed.
3. **A master build refills the mirror itself.** Anything it had to fetch from the buildbot or the
   install gets uploaded to `core-mirror-1`, so later builds read from the mirror again.

## Where each emulator comes from in the next build (checked with real downloads, 16 October)

- **11 from the buildbot**, which still has exactly the pinned build, only re-zipped: WonderSwan,
  Neo Geo Pocket, PC Engine CD, SuperGrafx, DOS, Lynx, 7800, 5200, Pokémon Mini, Saturn (Yabause),
  Saturn (Beetle).
- **5 from build 185's install**:
  - DOOM (prboom e5db549)
  - Jaguar (v3.6.1 f9a3c89)
  - Virtual Boy (v1.31.0 83ed426)
  - C64 (3.10 f63b566)
  - Amiga (5.3.1 7387937)

  The buildbot now serves the builds turned down on 12 October, plus rebuilt copies of the same
  C64 and Amiga code (the Amiga one was rebuilt on 15 October, after the hand-off was written).
  These five cores' dylib pins are now the sha256 of the 185 copies. Their zip pins are unchanged,
  so players get no false "emulator updated" heads-up.
- **`prboom.wad`** from its original URL, which still matches its pin byte for byte.

## How it's proven

- `bash scripts/check-core-mirror.sh` runs 41 checks on made-up files, with no network. It covers:
  an exact copy, a re-zip, a dylib in a folder, a different dylib, a broken zip, a missing copy,
  the install fallback (used, refused, missing, can't be downloaded, not downloaded when it isn't
  needed), the wad, and `fetch_one` end to end, including the self-refill upload.
- `bash scripts/check-core-mirror.sh live`: all 17 files come out as the pinned build, from real
  sources.
- A full real run of `scripts/fetch-buildbot-cores.sh` on Linux finished with all 16 staged as
  `pinned`, no warnings. The 185 copies pass every staging check: all 20 entry points are there,
  the install name is already `@rpath/<name>`, and the staged file isn't changed.
  `package-ipa.sh` ad-hoc signs them the same way again.
- `cargo test core_updates`: 12 passed (zip pins untouched).
- `scripts/check-core-updates.sh` skip list: C64 and Amiga updated for their same-code rebuilds.

## Why the mirror wasn't refilled by hand

This sandbox can't upload release files: the GitHub gateway rejects every asset upload
(HTTP 400). That includes `gh release upload` (GraphQL) and the plain REST upload endpoint. The
build server has a normal token, so the next master build does the upload itself (step 3 above).
After that build, `scripts/check-core-mirror.sh live` should report all 16 as "from the mirror".
