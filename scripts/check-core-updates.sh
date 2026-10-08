#!/usr/bin/env bash
# Ask every emulator's own repository whether it has moved on since the version Continuum is
# locked to, and write the answer to docs/CORE_UPDATES.md in plain English.
#
# WHY THIS EXISTS
# ---------------
# Every core is pinned to one exact upstream commit, deliberately: an upstream change landing
# overnight must not be able to alter or break a build nobody touched. The cost of that is
# that nothing tells anyone when an emulator HAS improved — the pins just sit there, quietly
# ageing, until a person happens to look. Nobody was looking.
#
# The owner is not going to track seventeen other people's repositories, and should not have
# to. So this runs on a schedule, writes one short page, and commits it. Whoever picks the
# project up next reads that page and tells the owner in plain English whether anything is
# worth taking. See HANDOFF.md.
#
# THIS SCRIPT CHANGES NOTHING ABOUT THE APP. It does not move a pin, does not build, does not
# touch a core. Taking an update is still a deliberate edit to scripts/build-core.sh followed
# by a build and a test on a phone. All this does is answer "is there anything new?".
#
# Needs: gh (authenticated), git. Runs anywhere; no macOS host needed.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/docs/CORE_UPDATES.md"
NOW="$(date -u '+%d %B %Y, %H:%M UTC')"

# repo + pin, deduplicated: the _jit builds share a repository and a pin with the core they are
# named after, so asking GitHub twice for the same answer would be wasted.
#
# Captured first rather than read straight from a process substitution, because bash ignores a
# process substitution's exit status: a build-core.sh that failed, or printed nothing, left ROWS
# empty, and the page then said "All 0 are up to date". That is the one answer this page must
# never give wrongly, so no pins fails the run loudly and leaves the page as it was.
if ! PINS="$(bash "$ROOT/scripts/build-core.sh" ios-pins)"; then
  echo "::error::scripts/build-core.sh ios-pins failed; $OUT was not rewritten"
  exit 1
fi
if [[ -z "$PINS" ]]; then
  echo "::error::scripts/build-core.sh ios-pins listed no cores; $OUT was not rewritten"
  exit 1
fi

declare -a ROWS=()
seen=""
while IFS=$'\t' read -r core repo pin display; do
  key="$repo@$pin"
  case " $seen " in *" $key "*) continue ;; esac
  seen="$seen $key"
  ROWS+=("$core"$'\t'"$repo"$'\t'"$pin"$'\t'"$display")
done <<<"$PINS"

echo "==> checking ${#ROWS[@]} emulator repositories against their pins"

behind_rows=""
current_rows=""
failed_rows=""
behind_count=0

for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r core repo pin display <<<"$row"
  # https://github.com/<owner>/<name> -> <owner>/<name>
  slug="${repo#https://github.com/}"
  slug="${slug%.git}"

  # Every call is allowed to fail on its own. A repository that was renamed, went private or
  # is simply unreachable today must produce a line saying so, not an empty page.
  if ! branch="$(gh api "repos/$slug" --jq '.default_branch' 2>/dev/null)" || [[ -z "$branch" ]]; then
    echo "    $core: could not reach $slug"
    failed_rows="$failed_rows| $display | \`$slug\` | could not reach it |"$'\n'
    continue
  fi

  if ! ahead="$(gh api "repos/$slug/compare/$pin...$branch" --jq '.ahead_by' 2>/dev/null)"; then
    echo "    $core: could not compare against $branch"
    failed_rows="$failed_rows| $display | \`$slug\` | pinned commit not found upstream (rebased or force-pushed?) |"$'\n'
    continue
  fi

  # Guarded like the two calls above. This one was not, and under `set -euo pipefail` a single
  # failed request here ended the whole run with the page not updated. The date is only a
  # detail of the row, so a failure makes it "unknown" rather than dropping the row.
  if ! newest="$(gh api "repos/$slug/commits/$branch" --jq '.commit.committer.date' 2>/dev/null)"; then
    echo "    $core: could not read the date of the newest change on $branch"
    newest=""
  fi
  newest="${newest:0:10}"
  [[ -n "$newest" ]] || newest="unknown"

  if [[ "$ahead" -gt 0 ]]; then
    echo "    $core: $ahead new change(s) upstream, newest $newest"
    behind_count=$((behind_count + 1))
    behind_rows="$behind_rows| **$display** | $ahead | $newest | \`${pin:0:8}\` |"$'\n'
  else
    echo "    $core: up to date"
    current_rows="$current_rows| $display | \`${pin:0:8}\` |"$'\n'
  fi
done

total="${#ROWS[@]}"

{
  echo "# Have any of the emulators been updated?"
  echo
  echo "Checked automatically, weekly. **Last checked: $NOW.**"
  echo
  echo "Written by \`scripts/check-core-updates.sh\`. Do not edit by hand; it is overwritten."
  echo
  echo "## What this page is, and is not"
  echo
  echo "Each emulator in Continuum is locked to one exact version of its author's code. That is"
  echo "deliberate: if the build took whatever was newest, somebody else's change overnight could"
  echo "break the app with nothing on this side having changed. The cost is that nothing announces"
  echo "when an emulator has genuinely improved. This page is that announcement."
  echo
  echo "**Nothing here has been taken.** Reading this page changes nothing about the app. Moving a"
  echo "core to a newer version is a deliberate edit to \`scripts/build-core.sh\`, then a build, then"
  echo "a test on a phone. A number in the table below is an invitation to look, not a problem."
  echo
  if [[ "$behind_count" -gt 0 ]]; then
    echo "## $behind_count of $total have newer code available"
    echo
    echo "\"New changes\" counts every commit the author has made since our locked version. A big"
    echo "number is not automatically important — it can be one real fix among a hundred tidy-ups."
    echo
    echo "| Console | New changes since ours | Newest change | Our locked version |"
    echo "| --- | --- | --- | --- |"
    printf '%s' "$behind_rows"
    echo
  else
    echo "## All $total are up to date"
    echo
    echo "Every emulator is locked to its author's newest code. Nothing to do."
    echo
  fi
  if [[ -n "$current_rows" ]]; then
    echo "## Already on the newest code"
    echo
    echo "| Console | Our locked version |"
    echo "| --- | --- |"
    printf '%s' "$current_rows"
    echo
  fi
  if [[ -n "$failed_rows" ]]; then
    echo "## Could not be checked"
    echo
    echo "Worth a look: a repository that cannot be reached, or a pinned commit that no longer"
    echo "exists upstream, means the next build of that core could fail for a reason that has"
    echo "nothing to do with this project."
    echo
    echo "| Console | Repository | What happened |"
    echo "| --- | --- | --- |"
    printf '%s' "$failed_rows"
    echo
  fi
} > "$OUT"

echo "==> wrote $OUT ($behind_count of $total have newer code)"
