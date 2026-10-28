#!/usr/bin/env bash
# Ask every emulator's upstream whether it has moved on since the version Continuum is locked
# to, and write the answer to docs/CORE_UPDATES.md in plain English.
#
# WHY THIS EXISTS
# ---------------
# Every core is pinned to one exact version, deliberately: the ones built here from source to an
# upstream commit (scripts/build-core.sh), and the ones downloaded from the libretro buildbot to
# the sha256 of their file (scripts/fetch-buildbot-cores.sh). An upstream change landing
# overnight must not be able to alter or break a build nobody touched. The cost of that is that
# nothing tells anyone when an emulator HAS improved — the pins just sit there, quietly ageing,
# until a person happens to look. Nobody was looking.
#
# The owner is not going to track every emulator's repository and the buildbot, and should not
# have to. So this runs on a schedule, writes one short page, and commits it. Whoever picks the
# project up next reads that page and tells the owner in plain English whether anything is
# worth taking. See .kiro/notes/HANDOFF.md.
#
# For each core built here it reports how many changes upstream has made, the newest change's
# date and subjects, and whether our patches (scripts/patches/) still apply to upstream's newest
# code. For each downloaded core it reports whether the buildbot's build differs from ours. A
# newer version that was already checked and turned down is listed as "checked and skipped"
# (checked_skip below) rather than reported as new again.
#
# THIS SCRIPT CHANGES NOTHING ABOUT THE APP. It does not move a pin, does not build, does not
# touch a core. Taking an update is still a deliberate edit to scripts/build-core.sh or
# scripts/fetch-buildbot-cores.sh followed by a build and a test on a phone. All this does is
# answer "is there anything new?". Its scratch (upstream checkouts, downloads) is under
# .work/update-check/.
#
# Needs: gh (authenticated), git, patch, curl, unzip, and sha256sum or shasum. Runs anywhere; no
# macOS host needed. Bash 3.2 safe.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/docs/CORE_UPDATES.md"
NOW="$(date -u '+%d %B %Y, %H:%M UTC')"
SCRATCH="$ROOT/.work/update-check"
BUILDBOT="https://buildbot.libretro.com/nightly/apple/ios-arm64/latest"

# A newer version that was checked and turned down: "<commit or dylib sha256>|<why>|<when>".
# A from-source core is matched on upstream's newest commit, a downloaded one on the sha256 of the
# buildbot's dylib. When upstream is still exactly that version the core is listed under "Checked
# and skipped" and not counted as new. When it has moved past it, the core is reported as new
# again, with this note beside it.
checked_skip() {
  case "$1" in
    gearsystem)
      echo "426d4eb61d6e2b3cc5d9431ed9cc3cb9efa1829b|only the desktop app and the file-based save code changed; the emulator Continuum builds is the same|16 October 2026" ;;
    mednafen_psx_hw)
      echo "e57860d82faa953dc0fc10f201fd583739bc2f7d|3-7% slower on the software renderer the app uses|12 October 2026" ;;
    mednafen_pce_fast)
      echo "4eee99c451f85ebe66d1591de88b892015b23059|only the Spanish and Swedish translations of its settings changed, and Continuum always shows them in English; the emulator Continuum builds is the same|27 October 2026" ;;
    blastem)
      echo "2230addc4dd74dd094fd264987fd808abba32d0e|only its makers' web-browser build changed (a new build job, and two network functions renamed for it, which Continuum never uses); the emulator Continuum builds runs the same code|27 October 2026" ;;
    prboom)
      echo "d14d20939640690b9fcef466e10038a9e630c70a39c173f6f048f823cbd5d060|turns the default sound and music volume to max, so DOOM’s sound clips|12 October 2026" ;;
    virtualjaguar)
      echo "024c4337de34938c75f6d783f8355de91ad30c10539f2056991ad75213b1adc9|about 13% slower|12 October 2026" ;;
    mednafen_vb)
      echo "2ee0c2c8e4c77073045ac8e135886f628c5ae93b962256e2ffa8db39f8147bad|about 19% slower|12 October 2026" ;;
    vice_x64sc)
      echo "e842ed6ff9bfd4eddef5dd4ea58592df1304070816d01589d8d188bdeff499b7|same code as ours (f63b566); only the build date inside differs|16 October 2026" ;;
    puae)
      echo "2cf44f8e5ad3fc1b9f1877ba4d0346088fbcdbd9ab0a74e802261677ad89c4cb|same code as ours (5.3.1, 7387937); only rebuilt by the buildbot|16 October 2026" ;;
    *) echo "" ;;
  esac
}

# The sha256 of a file, or of stdin when there is no argument, as just the hex.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum ${1:+"$1"} | cut -d' ' -f1
  else
    shasum -a 256 ${1:+"$1"} | cut -d' ' -f1
  fi
}

# Which scripts/patches/<family>-*.patch files a core applies: the same rule as
# ios_core_patch_family in scripts/build-core.sh.
patch_family() {
  local family="${1%_jit}"
  case "$family" in continuum_symbian) family="continuum-symbian" ;; esac
  echo "$family"
}

# The patch files of one core, one path per line (nothing when it has none).
core_patches() {
  local family p
  family="$(patch_family "$1")"
  for p in "$ROOT/scripts/patches/$family"-*.patch; do
    if [[ -f "$p" ]]; then echo "$p"; fi
  done
}

# The skip note for a core that moved past a checked-and-skipped version.
skip_note() {
  local value="$1" reason="$2" when="$3"
  echo "skipped \`${value:0:8}\` on $when: $reason; newer code since"
}

# Do our patches still fit upstream's newest code? Prints "yes", "no (<core>, ...)" or
# "couldn't check". Arguments: slug, branch, then the cores sharing that repository and pin.
patch_fit() {
  local slug="$1" branch="$2"
  shift 2
  local dir="$SCRATCH/${slug//\//_}"
  local core p path sm copy failed="" paths="" log
  rm -rf "$dir"
  mkdir -p "$dir"
  log="$dir.log"
  : > "$log"
  if ! { git -C "$dir" init -q \
         && git -C "$dir" remote add origin "https://github.com/$slug" \
         && git -C "$dir" fetch -q --depth 1 origin "$branch" \
         && git -C "$dir" -c advice.detachedHead=false checkout -q FETCH_HEAD; } </dev/null >>"$log" 2>&1; then
    rm -rf "$dir"
    echo "couldn't check"
    return 0
  fi
  # Only the submodules a patched file falls under, at depth 1: a whole recursive tree is
  # gigabytes for some cores, and nothing else is needed to know whether a hunk still fits.
  for core in "$@"; do
    while IFS= read -r p; do
      [[ -n "$p" ]] || continue
      paths="$paths$(sed -n 's|^+++ b/\([^[:space:]]*\).*|\1|p' "$p")"$'\n'
    done <<<"$(core_patches "$core")"
  done
  if [[ -f "$dir/.gitmodules" ]]; then
    while IFS= read -r sm; do
      [[ -n "$sm" ]] || continue
      case $'\n'"$paths" in
        *$'\n'"$sm"/*)
          if ! git -C "$dir" submodule update -q --init --depth 1 -- "$sm" </dev/null >>"$log" 2>&1; then
            rm -rf "$dir"
            echo "couldn't check"
            return 0
          fi
          ;;
      esac
    done <<<"$(git -C "$dir" config -f .gitmodules --get-regexp '^submodule\..*\.path$' 2>/dev/null \
                 | cut -d' ' -f2-)"
  fi
  for core in "$@"; do
    [[ -n "$(core_patches "$core")" ]] || continue
    copy="$dir.$core"
    rm -rf "$copy"
    if ! cp -a "$dir" "$copy"; then
      rm -rf "$copy" "$dir"
      echo "couldn't check"
      return 0
    fi
    if ! bash "$ROOT/scripts/build-core.sh" ios-apply-patches "$core" "$copy" </dev/null >>"$log" 2>&1; then
      failed="${failed:+$failed, }$core"
    fi
    rm -rf "$copy"
  done
  rm -rf "$dir"
  if [[ -n "$failed" ]]; then
    echo "no ($failed)"
  else
    echo "yes"
  fi
}

# Captured first rather than read straight from a process substitution, because bash ignores a
# process substitution's exit status: a build-core.sh that failed, or printed nothing, left the
# list empty, and the page then said "All 0 are up to date". That is the one answer this page
# must never give wrongly, so no pins fails the run loudly and leaves the page as it was. The
# same goes for the downloaded cores' pins.
if ! PINS="$(bash "$ROOT/scripts/build-core.sh" ios-pins)"; then
  echo "::error::scripts/build-core.sh ios-pins failed; $OUT was not rewritten"
  exit 1
fi
if [[ -z "$PINS" ]]; then
  echo "::error::scripts/build-core.sh ios-pins listed no cores; $OUT was not rewritten"
  exit 1
fi
if ! BB_PINS="$(bash "$ROOT/scripts/fetch-buildbot-cores.sh" pins)"; then
  echo "::error::scripts/fetch-buildbot-cores.sh pins failed; $OUT was not rewritten"
  exit 1
fi
if [[ -z "$BB_PINS" ]]; then
  echo "::error::scripts/fetch-buildbot-cores.sh pins listed no cores; $OUT was not rewritten"
  exit 1
fi

# repo + pin, grouped: the _jit builds share a repository and a pin with the core they are named
# after, so asking GitHub twice for the same answer would be wasted. Parallel arrays rather than
# an associative one (bash 3.2); never empty, because the pins above are not.
G_KEY=()
G_CORES=()
G_REPO=()
G_PIN=()
G_DISPLAY=()
while IFS=$'\t' read -r core repo pin display; do
  [[ -n "$core" ]] || continue
  key="$repo@$pin"
  found=-1
  i=0
  while [[ "$i" -lt "${#G_KEY[@]}" ]]; do
    if [[ "${G_KEY[$i]}" == "$key" ]]; then found="$i"; break; fi
    i=$((i + 1))
  done
  if [[ "$found" -ge 0 ]]; then
    G_CORES[$found]="${G_CORES[$found]} $core"
  else
    G_KEY+=("$key")
    G_CORES+=("$core")
    G_REPO+=("$repo")
    G_PIN+=("$pin")
    G_DISPLAY+=("$display")
  fi
done <<<"$PINS"

BB=()
while IFS= read -r line; do
  [[ -n "$line" ]] || continue
  BB+=("$line")
done <<<"$BB_PINS"

mkdir -p "$SCRATCH"
echo "==> checking ${#G_KEY[@]} emulator repositories and ${#BB[@]} buildbot cores"

behind_rows=""
bb_behind_rows=""
subject_lists=""
current_rows=""
skipped_rows=""
failed_rows=""
behind_count=0
failed_count=0
skipped_count=0

i=0
while [[ "$i" -lt "${#G_KEY[@]}" ]]; do
  cores="${G_CORES[$i]}"
  repo="${G_REPO[$i]}"
  pin="${G_PIN[$i]}"
  display="${G_DISPLAY[$i]}"
  first="${cores%% *}"
  i=$((i + 1))
  # https://github.com/<owner>/<name> -> <owner>/<name>
  slug="${repo#https://github.com/}"
  slug="${slug%.git}"

  # Every call is allowed to fail on its own. A repository that was renamed, went private or
  # is simply unreachable today must produce a line saying so, not an empty page.
  if ! branch="$(gh api "repos/$slug" --jq '.default_branch' 2>/dev/null)" || [[ -z "$branch" ]]; then
    echo "    $first: could not reach $slug"
    failed_rows="$failed_rows| $display | \`$slug\` | could not reach it |"$'\n'
    failed_count=$((failed_count + 1))
    continue
  fi

  if ! ahead="$(gh api "repos/$slug/compare/$pin...$branch" --jq '.ahead_by' 2>/dev/null)" \
      || [[ ! "$ahead" =~ ^[0-9]+$ ]]; then
    echo "    $first: could not compare against $branch"
    failed_rows="$failed_rows| $display | \`$slug\` | pinned commit not found upstream (rebased or force-pushed?) |"$'\n'
    failed_count=$((failed_count + 1))
    continue
  fi

  # Guarded like the two calls above. Under `set -euo pipefail` a single failed request here
  # would end the whole run with the page not updated. The date is only a detail of the row, so
  # a failure makes it "unknown" rather than dropping the row.
  if ! head_info="$(gh api "repos/$slug/commits/$branch" --jq '.sha + " " + .commit.committer.date' 2>/dev/null)"; then
    echo "    $first: could not read the newest change on $branch"
    head_info=""
  fi
  head_sha="${head_info%% *}"
  newest="${head_info#* }"
  newest="${newest:0:10}"
  [[ -n "$newest" ]] || newest="unknown"

  if [[ "$ahead" -eq 0 ]]; then
    echo "    $first: up to date"
    current_rows="$current_rows| $display | built here | \`${pin:0:8}\` |"$'\n'
    continue
  fi

  skip="$(checked_skip "$first")"
  skip_value="${skip%%|*}"
  note=""
  if [[ -n "$skip" ]]; then
    rest="${skip#*|}"
    skip_reason="${rest%%|*}"
    skip_when="${rest#*|}"
    if [[ -n "$head_sha" && "$head_sha" == "$skip_value" ]]; then
      echo "    $first: upstream is the version checked and skipped on $skip_when"
      skipped_rows="$skipped_rows| $display | \`${skip_value:0:8}\` ($ahead new changes) | $skip_reason | $skip_when |"$'\n'
      skipped_count=$((skipped_count + 1))
      continue
    fi
    note=" ($(skip_note "$skip_value" "$skip_reason" "$skip_when"))"
  fi

  echo "    $first: $ahead new change(s) upstream, newest $newest"
  behind_count=$((behind_count + 1))

  # The five newest change subjects: first line only, without | and backticks (they would break
  # the table and the code spans), at most 80 characters.
  if subjects="$(gh api "repos/$slug/compare/$pin...$branch" \
        --jq '.commits[-5:][] | .commit.message | split("\n")[0] | gsub("[|`]"; "") | .[0:80]' 2>/dev/null)" \
      && [[ -n "$subjects" ]]; then
    subject_lists="$subject_lists- **$display**"$'\n'
    while IFS= read -r s; do
      [[ -n "$s" ]] || continue
      subject_lists="$subject_lists  - $s"$'\n'
    done <<<"$subjects"
  else
    subject_lists="$subject_lists- **$display**"$'\n'"  - (could not read the change subjects)"$'\n'
  fi

  # Do our patches still fit? Only for a repository with a patched core on it.
  fit="—"
  for core in $cores; do
    if [[ -n "$(core_patches "$core")" ]]; then
      echo "    $first: trying our patches on upstream's newest code"
      # shellcheck disable=SC2086
      fit="$(patch_fit "$slug" "$branch" $cores)"
      break
    fi
  done

  behind_rows="$behind_rows| **$display**$note | $ahead | $newest | $fit | \`${pin:0:8}\` |"$'\n'
done

# The downloaded cores. The buildbot re-zips its builds nightly, so the zip's sha256 changes
# with nothing new in it: the dylib inside is what is compared (the same rule as the dylib pins
# in scripts/fetch-buildbot-cores.sh). No dates either: a re-zip date would change the page,
# and so make a commit, every week.
mkdir -p "$SCRATCH/buildbot"
for line in "${BB[@]}"; do
  IFS=$'\t' read -r core zip_pin dylib_pin label <<<"$line"
  name="${label//-/ }"
  zip="$SCRATCH/buildbot/${core}_libretro_ios.dylib.zip"
  rm -f "$zip"
  live=""
  if curl -fsSL --retry 2 -o "$zip" "$BUILDBOT/${core}_libretro_ios.dylib.zip" 2>/dev/null; then
    live="$(unzip -p "$zip" "${core}_libretro_ios.dylib" 2>/dev/null | sha256_of)" || live=""
    # sha256 of nothing: the entry was missing from the zip.
    [[ "$live" != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" ]] || live=""
  fi
  rm -f "$zip"
  if [[ ! "$live" =~ ^[0-9a-f]{64}$ ]]; then
    echo "    $core: couldn't download it from the buildbot"
    failed_rows="$failed_rows| $name | the libretro buildbot | couldn't download |"$'\n'
    failed_count=$((failed_count + 1))
    continue
  fi
  if [[ "$live" == "$dylib_pin" ]]; then
    echo "    $core: same as ours"
    current_rows="$current_rows| $name | downloaded | \`${dylib_pin:0:8}\` |"$'\n'
    continue
  fi
  skip="$(checked_skip "$core")"
  note=""
  if [[ -n "$skip" ]]; then
    skip_value="${skip%%|*}"
    rest="${skip#*|}"
    skip_reason="${rest%%|*}"
    skip_when="${rest#*|}"
    if [[ "$live" == "$skip_value" ]]; then
      echo "    $core: the buildbot has the build checked and skipped on $skip_when"
      skipped_rows="$skipped_rows| $name | \`${skip_value:0:8}\` (buildbot build) | $skip_reason | $skip_when |"$'\n'
      skipped_count=$((skipped_count + 1))
      continue
    fi
    note=" ($(skip_note "$skip_value" "$skip_reason" "$skip_when"))"
  fi
  echo "    $core: newer build on the buildbot (${live:0:8})"
  behind_count=$((behind_count + 1))
  bb_behind_rows="$bb_behind_rows| **$name**$note | newer build (\`${live:0:8}\`) | \`${dylib_pin:0:8}\` |"$'\n'
done

total=$(( ${#G_KEY[@]} + ${#BB[@]} ))

{
  echo "# Have any of the emulators been updated?"
  echo
  echo "Checked automatically, weekly. **Last checked: $NOW.**"
  echo
  echo "Written by \`scripts/check-core-updates.sh\`. Do not edit by hand; it is overwritten."
  echo
  echo "## What this page is, and is not"
  echo
  echo "Each emulator in Continuum is locked to one exact version. That is deliberate: if the build"
  echo "took whatever was newest, somebody else's change overnight could break the app with nothing"
  echo "on this side having changed. The cost is that nothing announces when an emulator has"
  echo "genuinely improved. This page is that announcement."
  echo
  echo "It covers every emulator: the ones built here from their author's code, and the ones"
  echo "downloaded ready-made from the libretro buildbot. For the ones built here it also checks"
  echo "whether our own patches still fit the author's newest code, so taking an update would not"
  echo "start with a broken build."
  echo
  echo "**Nothing here has been taken.** Reading this page changes nothing about the app. Moving a"
  echo "core to a newer version is a deliberate edit to \`scripts/build-core.sh\` (or"
  echo "\`scripts/fetch-buildbot-cores.sh\` for a downloaded one), then a build, then a test on a"
  echo "phone. A number in the tables below is an invitation to look, not a problem."
  echo
  if [[ "$behind_count" -gt 0 ]]; then
    echo "## $behind_count of $total have newer code available"
    echo
    if [[ -n "$behind_rows" ]]; then
      echo "Built here. \"New changes\" counts every commit the author has made since our locked"
      echo "version. A big number is not automatically important — it can be one real fix among a"
      echo "hundred tidy-ups."
      echo
      echo "| Console | New changes since ours | Newest change | Our patches still fit | Our locked version |"
      echo "| --- | --- | --- | --- | --- |"
      printf '%s' "$behind_rows"
      echo
    fi
    if [[ -n "$bb_behind_rows" ]]; then
      echo "Downloaded. The buildbot's build is a different file from ours; that alone does not say"
      echo "what changed in it."
      echo
      echo "| Console | What the buildbot has | Our locked build |"
      echo "| --- | --- | --- |"
      printf '%s' "$bb_behind_rows"
      echo
    fi
    if [[ -n "$subject_lists" ]]; then
      echo "The newest changes, as their authors describe them:"
      echo
      printf '%s' "$subject_lists"
      echo
    fi
  elif [[ "$failed_count" -eq 0 ]]; then
    echo "## All $total are up to date"
    echo
    if [[ "$skipped_count" -gt 0 ]]; then
      echo "Nothing new to take: $((total - skipped_count)) are on their newest version, and the other"
      echo "$skipped_count have a newer one that was already checked and skipped (below)."
    else
      echo "Every emulator is on its newest version. Nothing to do."
    fi
    echo
  else
    # Not "all up to date": the ones that could not be checked are unknown, not current.
    echo "## None of the $((total - failed_count)) checked have newer code; $failed_count could not be checked"
    echo
  fi
  if [[ -n "$current_rows" ]]; then
    echo "## Already on the newest code"
    echo
    echo "| Console | Comes from | Our locked version |"
    echo "| --- | --- | --- |"
    printf '%s' "$current_rows"
    echo
  fi
  if [[ -n "$skipped_rows" ]]; then
    echo "## Checked and skipped"
    echo
    echo "A newer version exists, and it was tried and turned down. These are not counted as new"
    echo "until the author moves past the version named here."
    echo
    echo "| Console | Newer version | Why it was skipped | When |"
    echo "| --- | --- | --- | --- |"
    printf '%s' "$skipped_rows"
    echo
  fi
  if [[ -n "$failed_rows" ]]; then
    echo "## Could not be checked"
    echo
    echo "Worth a look: a repository that cannot be reached, a pinned commit that no longer exists"
    echo "upstream, or a download that fails means the next build of that core could fail for a"
    echo "reason that has nothing to do with this project."
    echo
    echo "| Console | Where | What happened |"
    echo "| --- | --- | --- |"
    printf '%s' "$failed_rows"
    echo
  fi
} > "$OUT"

echo "==> wrote $OUT ($behind_count of $total have newer code; $skipped_count checked and skipped; $failed_count could not be checked)"
