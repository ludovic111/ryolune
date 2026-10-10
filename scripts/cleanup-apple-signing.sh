#!/usr/bin/env bash
# Undo scripts/prepare-apple-signing.sh, whatever happened before (the release workflow runs it
# with `if: always()`): put the user's keychain search list back exactly as it was, delete the
# throwaway keychain and the notarization key. The default keychain is never touched.
set -euo pipefail
: "${RUNNER_TEMP:?}"
if [ -f "$RUNNER_TEMP/keychains-before" ]; then
  before=()
  while IFS= read -r line; do before+=("$line"); done < "$RUNNER_TEMP/keychains-before"
  security list-keychains -d user -s ${before[@]+"${before[@]}"}
  rm -f "$RUNNER_TEMP/keychains-before"
fi
if [ -f "$RUNNER_TEMP/signing.keychain-db" ]; then
  security delete-keychain "$RUNNER_TEMP/signing.keychain-db"
fi
rm -f "$RUNNER_TEMP/notary.p8" "$RUNNER_TEMP/developer-id.p12"
