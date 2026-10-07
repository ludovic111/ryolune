# ryolune documentation

Every document in `docs/`, one line each, with who it is for. The project overview is the
[README](../README.md) at the repository root.

## Using ryolune

- [USER_GUIDE.md](USER_GUIDE.md): everything you can do in the window, from a first beat to an
  exported mix. For musicians.
- [SHORTCUTS.md](SHORTCUTS.md): every keyboard shortcut. Generated from the app's shortcut
  sheet. For musicians.
- [PLUGINS.md](PLUGINS.md): the plugin formats ryolune hosts (VST3, CLAP, Audio Units, native),
  where it looks for them and how they behave. For musicians and plugin users.

## Controlling it (CLI, MCP, agents)

- [AI_CONTROL.md](AI_CONTROL.md): the four ways in (window, `ryolune-cli`, `ryolune-mcp`, the
  built-in agent), what to call first, and the generation contract. For script authors and
  anyone connecting an AI.
- [AGENT.md](AGENT.md): setting up and using the built-in agent panel. For musicians who use
  the agent.
- [COMMANDS.md](COMMANDS.md): every registry command and its parameters. Generated from the
  registry. For script and agent authors.
- [HARNESS.md](HARNESS.md): the agent's brief and its 13 skills, exactly as every agent gets
  them. Generated from `engine/src/harness`. For agent authors and anyone tuning the agent.
- [../evals/README.md](../evals/README.md): the agent evals (scripted music jobs scored on the
  song) and their results. For contributors changing the harness.
- [AGENT_PARITY.md](AGENT_PARITY.md): the audit that maps every window interaction to the
  commands that do the same. For contributors and agent authors.
- [agent-parity.json](agent-parity.json): the machine-readable map from each window action id
  to its commands, checked by `engine/tests/agent_parity.rs`. For contributors.

## Building it

- [DEVELOPMENT.md](DEVELOPMENT.md): layout, building, checking and publishing a release. For
  contributors.
- [ARCHITECTURE.md](ARCHITECTURE.md): how the crates, threads, registry, store, audio engine,
  plugin hosts and window fit together. For contributors.
- [NATIVE_PLUGINS.md](NATIVE_PLUGINS.md): writing a native plugin with the `ryolune-plugin`
  SDK and its C ABI. For plugin developers.
- [NEXT_SESSION.md](NEXT_SESSION.md): a hand-off prompt from the 0.10 work, kept for its
  open questions. For maintainers.

## Reference

- [CONFIGURATION.md](CONFIGURATION.md): every setting, the data folder, the files ryolune
  writes, environment variables and command-line flags. For power users and contributors.
- [SESSION_FORMAT.md](SESSION_FORMAT.md): the `.ryolune` file format, field by field, with a
  minimal valid example. For tool authors and contributors.

## Releases and history

- [releases/](releases/): release notes for every version from 0.2.0 to 0.13.0. For everyone.
- [RUST_MIGRATION.md](RUST_MIGRATION.md): the 2026-09-12 move from the Electron app to Rust.
  History, for contributors.
- [VERIFICATION.md](VERIFICATION.md): what was checked by hand for 0.3.1, and the limits of
  that evidence. History, for maintainers.
- [verification/](verification/): older review notes and captures (agent setup, streaming,
  the former Aero look). History, for maintainers.

Outside `docs/`: [CONTRIBUTING.md](../CONTRIBUTING.md) and [CLA.md](../CLA.md) for
contributors, and `desktop/src/ui/README.md`, the contract for code in the window.
