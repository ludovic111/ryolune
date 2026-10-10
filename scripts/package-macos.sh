#!/usr/bin/env bash
# Wrap the release build into dist/ryolune.app and zip it as dist/ryolune-macos-<arch>.zip
# (ryolune-macos-arm64.zip, ryolune-macos-x86_64.zip), the asset name the in-app updater
# downloads. The bundle's version follows Cargo.toml.
#
#   scripts/package-macos.sh                                    # target/release, this Mac's arch
#   RYOLUNE_TARGET=x86_64-apple-darwin scripts/package-macos.sh # target/<triple>/release
#
# Intel builds are cross-compiled on Apple Silicon (cargo build --release --workspace --target
# x86_64-apple-darwin). Without APPLE_SIGNING_IDENTITY the bundle is ad-hoc signed.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
if [ -n "${RYOLUNE_TARGET:-}" ]; then
  release="target/$RYOLUNE_TARGET/release"
  machine=${RYOLUNE_TARGET%%-*}
else
  release=target/release
  machine=$(uname -m)
fi
case "$machine" in
  arm64|aarch64) arch=arm64 ;;
  x86_64) arch=x86_64 ;;
  *) echo "Unsupported architecture $machine" >&2; exit 1 ;;
esac
test -x "$release/ryolune" || { echo "Run cargo build --release --workspace${RYOLUNE_TARGET:+ --target $RYOLUNE_TARGET} first." >&2; exit 1; }
# Validate every companion before replacing an existing local package. A binary this Mac cannot
# run (Intel without Rosetta) is checked by its architecture and the version string inside it.
for binary in ryolune ryolune-cli ryolune-mcp; do
  test -x "$release/$binary" || { echo "Missing $binary; build the workspace first." >&2; exit 1; }
  lipo "$release/$binary" -verify_arch "$arch"
  if actual=$("$release/$binary" --version 2>/dev/null); then
    test "$actual" = "$binary $version" || { echo "$binary has stale version: $actual (expected $version)" >&2; exit 1; }
  else
    grep -qaF "$version" "$release/$binary" || { echo "$binary does not carry version $version" >&2; exit 1; }
    echo "$binary ($arch) cannot run on this Mac; checked its architecture and embedded version."
  fi
done
bundle='dist/ryolune.app'
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
for binary in ryolune ryolune-cli ryolune-mcp; do
  cp "$release/$binary" "$bundle/Contents/MacOS/$binary"
done
sed -e "s|<string>0\.0\.0</string>|<string>$version</string>|" desktop/Info.plist > "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" "$bundle/Contents/Info.plist"
plutil -lint "$bundle/Contents/Info.plist"
cp desktop/assets/ryolune.icns "$bundle/Contents/Resources/ryolune.icns"
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  # Developer ID: hardened runtime, secure timestamp, companions first and the bundle last.
  for binary in ryolune-cli ryolune-mcp ryolune; do
    target="$bundle/Contents/MacOS/$binary"
    test "$binary" = ryolune && target="$bundle"
    codesign --force --options runtime --timestamp --entitlements desktop/ryolune.entitlements \
      --sign "$APPLE_SIGNING_IDENTITY" "$target"
  done
  codesign --verify --deep --strict "$bundle"
  if [ -n "${APPLE_API_KEY_PATH:-}" ]; then
    # Notarize with an App Store Connect API key, then staple the ticket so the first launch
    # passes Gatekeeper offline.
    submission="dist/ryolune-notarize-$arch.zip"
    rm -f "$submission"
    ditto -c -k --keepParent "$bundle" "$submission"
    result=$(xcrun notarytool submit "$submission" --key "$APPLE_API_KEY_PATH" \
      --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" --wait --output-format json)
    rm -f "$submission"
    echo "$result"
    status=$(printf '%s' "$result" | python3 -c 'import json, sys; print(json.load(sys.stdin).get("status", ""))')
    if [ "$status" != Accepted ]; then
      id=$(printf '%s' "$result" | python3 -c 'import json, sys; print(json.load(sys.stdin).get("id", ""))')
      test -n "$id" && xcrun notarytool log "$id" --key "$APPLE_API_KEY_PATH" \
        --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" || true
      echo "Notarization was not accepted: $status" >&2
      exit 1
    fi
    xcrun stapler staple "$bundle"
    xcrun stapler validate "$bundle"
    spctl --assess --type execute --verbose=2 "$bundle"
  fi
else
  # Ad-hoc signing for local testing and self-updates when no Developer ID is configured.
  codesign --force --deep --sign - "$bundle"
  codesign --verify --deep --strict "$bundle"
fi
rm -f "dist/ryolune-macos-$arch.zip"
ditto -c -k --sequesterRsrc --keepParent "$bundle" "dist/ryolune-macos-$arch.zip"
echo "Built $bundle ($version, $arch) and dist/ryolune-macos-$arch.zip"
