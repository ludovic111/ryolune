# ryolune (unreleased)

Changes since 0.16.0, for the next release's notes.

## macOS is back

- **ryolune ships for Linux and macOS**: Macs with Apple Silicon (`ryolune-macos-arm64.zip`) and
  with Intel (`ryolune-macos-x86_64.zip`) get ready-made builds again, through the lsuite app and
  the in-app updater like Linux. Windows is coming soon.

## lsuite is fully free

- **No lsuite account any more**: everything in ryolune is free, with nothing to sign in to.
  lsuite AI, the lsuite account and its plans are gone: the `lsuite AI` service in Settings ›
  Agent and the first-run setup, Claude Code's **Run on lsuite AI** switch, and the
  `account.status`, `account.signIn`, `account.signOut`, `account.plans` and `account.manage`
  commands. Bring your own agent: Codex or Claude Code with their own sign-in, an API key
  (Anthropic, OpenAI, Gemini, OpenRouter, Mistral, Groq, DeepSeek, xAI), or a model on your
  computer (Ollama, LM Studio, any compatible server). If lsuite AI was your agent, ryolune starts
  on Codex and keeps the rest of your settings; choose another service in Settings › Agent.
- **Updates need no account**: ryolune asks lsuite.xyz for the latest version
  (`/api/apps/ryolune/releases/latest`) and downloads it without a token; every download is still
  checked against ryolune's signature before anything is replaced. `LSUITE_SERVER` names another
  lsuite server (it replaces `LSUITE_ACCOUNT_SERVER`). An `~/.lsuite/account.json` left by an
  older version is ignored, and left in place.

## Removed

- **The zenith provider is removed**: it is no longer in Settings › Agent, the setup or
  `agent.configure`. If it was your agent, ryolune starts on Codex and keeps the rest of your
  settings; choose another service in Settings › Agent.
