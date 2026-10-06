# Security policy

## Reporting a vulnerability

Please report a vulnerability privately through
[GitHub security advisories](https://github.com/ludovic111/ryolune/security/advisories/new),
not in a public issue. Say what is affected, how to reproduce it and the impact you see. You
will get an answer, and a fix ships in a signed update as soon as it is ready. Only the latest
release is supported: every update is free, so updating is the fix.

## What ryolune protects

- **API keys.** Keys for agent and sound generation services live in `settings.json` in the
  data folder, readable only by your user (mode 0600 on macOS and Linux). They are sent only
  to the service they belong to, are masked whenever settings are read back (`settings.get`,
  the Settings window), and are masked in the log and crash reports.
- **The local connection.** `ryolune-cli`, `ryolune-mcp` and outside agents reach the window
  over a loopback socket with a per-run token in a discovery file only your user can read.
  It can be turned off in Settings › Control. Agents get only what Settings › Agent allows;
  connections, keys and permissions are changed by a person, never by an agent.
- **Diagnostics.** Logs and crash reports stay on your computer (Settings › Diagnostics).
  Report a Problem opens a prefilled GitHub issue that you read and submit yourself; nothing
  is sent automatically.
- **Plugins.** Bundles are scanned in a separate process, so a plugin that crashes while
  being probed does not take ryolune down. Plugins run with your user's rights: install
  plugins you trust.

## How updates are signed

Every release is built by the public GitHub Actions workflow (`.github/workflows/release.yml`).
It writes `SHA256SUMS` for the release files and signs it with an Ed25519 key kept as a
repository secret; the public key is built into the app (`desktop/assets/update-signing.pub`).
Before an update replaces anything, ryolune checks that:

1. the download comes from this repository's GitHub releases,
2. `SHA256SUMS` carries a valid signature from that key,
3. the archive matches its checksum, and
4. the new binaries report the version that was announced.

If any check fails, nothing is replaced and the current copy keeps running. You can verify a
download yourself with `ryolune --verify-release SHA256SUMS`.
