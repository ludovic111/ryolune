#!/usr/bin/env bash
# Publish a ryolune build to lsuite (lsuite's DISTRIBUTION.md): the release workflow leaves a
# complete draft release `v<version>` in ludovic111/ryolune; this copies its files and notes to
# the private ludovic111/lsuite-builds as `ryolune-v<version>`, which lsuite.xyz serves to
# everyone (free, no account), then deletes the draft (the tag stays). Signatures are unchanged:
# the app checks SHA256SUMS.sig against the key built into it, whoever serves the files.
#
#   scripts/publish-build.sh 0.16.0
#
# Needs `gh` signed in with access to both repositories. RYOLUNE_REPO and LSUITE_BUILDS_REPO
# override the repositories (tests).
set -euo pipefail
version=${1:?Usage: scripts/publish-build.sh <version>}
version=${version#v}
case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "Not a version: $version" >&2; exit 2 ;;
esac
source_repo=${RYOLUNE_REPO:-ludovic111/ryolune}
builds_repo=${LSUITE_BUILDS_REPO:-ludovic111/lsuite-builds}
tag="v$version"
build_tag="ryolune-v$version"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

draft=$(gh release view "$tag" --repo "$source_repo" --json isDraft --jq .isDraft) || {
  echo "There is no release $tag in $source_repo (run the release workflow first)." >&2
  exit 1
}
if [ "$draft" != true ]; then
  echo "$tag in $source_repo is already public, not a draft; refusing to move it." >&2
  exit 1
fi
gh release view "$tag" --repo "$source_repo" --json body --jq .body > "$work/notes.md"
mkdir "$work/files"
gh release download "$tag" --repo "$source_repo" --dir "$work/files"
for required in SHA256SUMS SHA256SUMS.sig; do
  test -s "$work/files/$required" || { echo "The draft has no $required; refusing to publish it." >&2; exit 1; }
done
# Every file the checksums list is there and intact.
if command -v sha256sum > /dev/null; then
  (cd "$work/files" && sha256sum --check --quiet SHA256SUMS)
else
  (cd "$work/files" && shasum -a 256 --check --quiet SHA256SUMS)
fi

if gh release view "$build_tag" --repo "$builds_repo" --json tagName > /dev/null 2>&1; then
  # Already copied (an earlier run stopped before deleting the draft): only the same bytes.
  gh release download "$build_tag" --repo "$builds_repo" --pattern SHA256SUMS --output "$work/published-sums" --clobber
  cmp -s "$work/files/SHA256SUMS" "$work/published-sums" || {
    echo "$build_tag already exists in $builds_repo with different files. Publish changes under a new version." >&2
    exit 1
  }
  echo "$build_tag is already in $builds_repo with the same files."
else
  gh release create "$build_tag" --repo "$builds_repo" --title "ryolune $version" \
    --notes-file "$work/notes.md" "$work"/files/*
fi
gh release delete "$tag" --repo "$source_repo" --yes
echo "Published ryolune $version to $builds_repo as $build_tag; the draft in $source_repo is deleted."
