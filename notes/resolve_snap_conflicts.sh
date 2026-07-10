#!/usr/bin/env bash
# Figure out (and optionally resolve) merge conflicts in insta .snap files.
#
# For each conflicted .snap it reports whether ours/theirs actually differ
# (ignoring the insta metadata header), then — with --resolve — takes theirs
# and stages it, since snapshots get regenerated from the merged code anyway:
#
#   notes/resolve_snap_conflicts.sh --resolve
#   cargo insta test --accept
set -euo pipefail

resolve=false
[[ "${1:-}" == "--resolve" ]] && resolve=true

strip_header() {
  # Drop the insta frontmatter (--- ... --- at top) so only rendered content
  # is compared.
  awk 'BEGIN{h=0} /^---$/{h++; next} h!=1'
}

mapfile -t snaps < <(git diff --name-only --diff-filter=U -- '*.snap')

if [[ ${#snaps[@]} -eq 0 ]]; then
  echo "No conflicted .snap files."
  exit 0
fi

for f in "${snaps[@]}"; do
  if diff -q <(git show ":2:$f" | strip_header) \
             <(git show ":3:$f" | strip_header) >/dev/null 2>&1; then
    echo "HEADER-ONLY : $f"
  else
    echo "CONTENT DIFF: $f"
  fi
  if $resolve; then
    git checkout --theirs -- "$f"
    git add "$f"
  fi
done

if $resolve; then
  echo
  echo "Resolved ${#snaps[@]} snapshot(s) with --theirs and staged them."
  echo "Now regenerate from the merged code:  cargo insta test --accept"
else
  echo
  echo "Dry run only. Re-run with --resolve to take theirs and stage."
fi
