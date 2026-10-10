#!/usr/bin/env bash
# Upload only to a draft; published releases and their checksums are immutable.
# Since 0.16 (lsuite's DISTRIBUTION.md) the release stays a draft here: the builds are no longer
# public. `scripts/publish-build.sh <version>` copies the draft to ludovic111/lsuite-builds,
# which lsuite.xyz serves to everyone (free, no account), and deletes the draft.
set -euo pipefail
cd "$(dirname "$0")/.."
bash scripts/verify-release.sh
tag=${GITHUB_REF_NAME:?Missing release tag}
notes="docs/releases/${tag#v}.md"
# Linux only while lsuite is in beta; macOS and Windows are coming soon (release.yml).
assets=(ryolune-linux-x86_64.zip ryolune-linux-x86_64.tar.gz ryolune-linux-x86_64 ryolune-Afterglow-demo.zip)
for asset in "${assets[@]}" SHA256SUMS SHA256SUMS.sig; do
  test -s "dist/$asset" || { echo "Missing release asset: $asset" >&2; exit 1; }
done
(cd dist && sha256sum --check SHA256SUMS)
metadata=$(mktemp)
existing_sums=$(mktemp)
trap 'rm -f "$metadata" "$existing_sums"' EXIT
if gh release view "$tag" --json isDraft,assets > "$metadata"; then
  draft=$(python3 -c 'import json,sys; print(str(json.load(open(sys.argv[1]))["isDraft"]).lower())' "$metadata")
  if [ "$draft" != true ]; then
    # Rebuilding a signed archive can change its bytes. Fail, rather than replacing a
    # version people have installed. A new payload requires a new version/tag.
    gh release download "$tag" --pattern SHA256SUMS --output "$existing_sums" --clobber
    cmp -s dist/SHA256SUMS "$existing_sums" || {
      echo "$tag is already published with different checksums. Publish changes under a new version." >&2
      exit 1
    }
    python3 - "$metadata" "${assets[@]}" SHA256SUMS SHA256SUMS.sig <<'PY'
import json, sys
present = {asset['name'] for asset in json.load(open(sys.argv[1]))['assets']}
missing = set(sys.argv[2:]) - present
if missing:
    raise SystemExit('Published release is missing assets; repair requires explicit review: ' + ', '.join(sorted(missing)))
PY
    echo "$tag is already published with the same complete asset manifest; nothing changed."
    exit 0
  fi
else
  # A network/auth failure cannot overwrite anything: create will fail if the release exists.
  gh release create "$tag" --verify-tag --draft --title "ryolune ${tag#v}" --notes-file "$notes"
fi
uploads=(dist/SHA256SUMS dist/SHA256SUMS.sig)
for asset in "${assets[@]}"; do uploads+=("dist/$asset"); done
# Confirm it is still a draft immediately before the only replace operation.
test "$(gh release view "$tag" --json isDraft --jq .isDraft)" = true || {
  echo 'The release was published while this job was preparing; refusing to replace its assets.' >&2
  exit 1
}
gh release upload "$tag" "${uploads[@]}" --clobber
# The tag is checked again after uploading, before the draft is handed on.
bash scripts/verify-release.sh
gh release edit "$tag" --title "ryolune ${tag#v}" --notes-file "$notes"
echo "Draft $tag is complete. Publish it to lsuite with: scripts/publish-build.sh ${tag#v}"
