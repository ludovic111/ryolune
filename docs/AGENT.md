# The ryolune agent

ryolune can be driven four ways: the window, `ryolune-cli`, `ryolune-mcp` and the built-in
agent panel. All four call the same command registry (`ryolune-cli commands` lists it), so an
agent's edits are ordinary undo steps, appear in the Activity log with an Undo button, and are
marked in the arrangement with the accent colour.

## Connect and start a conversation

Open the Agent panel and choose **Set up agent**, or use its settings button. Both open
**Settings > Agent** directly. Choose an AI service; only the fields for that service appear.

- **Codex / Claude Code:** ryolune looks for the installed companion and checks its sign-in.
  If it is missing or outdated, use the installation help, install/update it, and choose
  **Check connection**. If it is installed but signed out, choose **Sign in**, finish in your
  browser, and return to ryolune. Existing CLI credentials stay with the provider. Fresh
  companion installation is still a separate step; ryolune does not install vendor software.
- **An API key** (Anthropic, OpenAI, Google Gemini, OpenRouter, Mistral, Groq, DeepSeek, xAI):
  paste a key and choose **Save connection**. These APIs have their own billing, separate from a
  chat subscription. Stored keys are never prefilled in the input; entering a replacement or
  removing a key is explicit. Services without a lasting default model (Groq, xAI) ask you to
  pick one of your models from the list the service gives.
- **On this computer** (Ollama, LM Studio): start the app, choose one of its models that supports
  tool calls, and save. Nothing leaves the machine. The address field stays empty unless the
  server listens somewhere other than its usual port.
- **Other compatible server:** enter the running server's address and its model name, add a key
  only if the server requires one, and choose **Save connection**. The model must support tool
  calls. Prompts go to that configured server, which may be local or remote.
- **Zenith · lsuite:** the agents you use in zenith, lsuite's agent hub, with the sign-ins you
  made there. Open zenith once so ryolune finds it; **Check connection** asks zenith for its
  providers and sends no prompt.

**Account connected** means the CLI reports a successful sign-in. **Ready to try** means an
API key or server configuration is available; it does not claim that network access, model
availability or billing has been verified. No model request is sent by **Check connection**.
The first message exercises actual model access. A disabled local bridge is reported rather
than silently re-enabled. Changing service resets the model override to the service default;
compatible servers require their model name again. Optional model/executable overrides, standing
instructions and limits remain in **Advanced connection settings**.

Choose **Start chatting** and describe what you want in your own language. The musical starter
buttons fill the composer for you to adapt; they never send automatically. Enter sends,
Shift+Enter inserts a newline, and Cmd/Ctrl+Enter also sends. Drafts survive collapsing the panel,
failed sends preserve the message, and repeated clicks cannot submit the same pending request twice.
Unsaved connection edits survive switching Settings sections; closing asks whether to discard them.

## Providers

Settings > Agent chooses how the agent thinks:

| Provider | Sign-in | Where prompts go |
| --- | --- | --- |
| Codex CLI | `codex login` (Settings has a button) | OpenAI, through the CLI |
| Claude Code CLI | `claude auth login` (Settings has a button) | Anthropic, through the CLI |
| Anthropic API | API key in Settings or `ANTHROPIC_API_KEY` | `api.anthropic.com` directly |
| OpenAI API | API key in Settings or `OPENAI_API_KEY` | `api.openai.com` directly |
| Google Gemini | API key or `GEMINI_API_KEY` / `GOOGLE_API_KEY` | Gemini's OpenAI-compatible endpoint |
| OpenRouter | API key or `OPENROUTER_API_KEY` | `openrouter.ai`, which routes to the model's lab |
| Mistral | API key or `MISTRAL_API_KEY` | `api.mistral.ai` |
| Groq | API key or `GROQ_API_KEY` | `api.groq.com` |
| DeepSeek | API key or `DEEPSEEK_API_KEY` | `api.deepseek.com` |
| xAI Grok | API key or `XAI_API_KEY` | `api.x.ai` |
| Ollama | none | `127.0.0.1:11434` on this computer |
| LM Studio | none | `127.0.0.1:1234` on this computer |
| Other compatible server | base URL and optional key | your server |
| Zenith · lsuite | the agents' sign-ins in zenith | zenith, then the agent chosen there |

The CLI providers run in their own process group and receive this window's ryolune tools.
Codex streams through app-server dynamic tools; Claude Code uses the local MCP bridge.
Personal shell tools, hooks and unrelated integrations are excluded from these music sessions. The API providers
stream replies and tool calls directly; keys live in `settings.json` (mode 0600) and are shown
masked everywhere, including `settings.get`. Every service after Anthropic speaks the OpenAI
Chat Completions API; Mistral and DeepSeek get the older `max_tokens` field and no stream-usage
option, which they would refuse.

**Zenith** runs through `zenith-cli` (found through `$RYOLUNE_ZENITH_CLI`, the path in Settings,
zenith's lsuite discovery entry `~/.lsuite/apps/zenith.json`, then PATH). Each song gets a folder,
`<data dir>/agent-workspaces/<song id>`, added to zenith as a project (`project.add`) and holding
ryolune's MCP recipe (`.mcp.json`). A conversation's first request starts a zenith thread
(`thread.new`) whose id ryolune chooses beforehand, so Stop can always interrupt it
(`thread.interrupt`, then ryolune waits until the thread has stopped); follow-ups go to the same
thread (`thread.send`). ryolune follows the thread (`thread.get`) and shows its reply as it grows;
when zenith waits for an approval or has a question, the status line says so: answer in zenith.
zenith's agent edits the song with ryolune's own MCP server (zenith hands its agents the server
named in ryolune's lsuite entry, and the song's folder has the same recipe), so every edit is an
undo step, listed in Changes and in the chat. The model picker lists zenith's models as
`provider/model` (`provider.list`); zenith keeps its own approval mode.

## Use another agent

**Settings > Agent > Use another agent** gives a ready configuration for agents that live outside
ryolune and connect to the open window over MCP (`ryolune-mcp --live` through the local bridge):
Claude Code and Codex (one terminal command), Cursor and VS Code (an **Add to…** button that opens
the app's install prompt), Claude Desktop, Gemini CLI, Windsurf, opencode, Zed and any other MCP
client (a JSON block and the file it goes in). They get every registry command the built-in agent
has, under the same permissions, and their edits land in Undo and Changes. `agent.mcp` returns the
same recipes to the CLI; `agent.openClient` opens an install link and only a person can call it.

## Permissions

Settings > Agent > Permissions gates what any agent (the panel, MCP clients, `ryolune-cli
--agent`) may do: file operations, transport, replacing the session, changing settings and
application control, and sound generation (on by default; it spends the generation service's
credits). Plain document edits are always allowed and always undoable. Agents can never change
the agent, control or generation settings: only a person can, in the window.

## Conversations and project memory

Each song keeps its own conversations, saved in `<data dir>/agent-conversations.json` and found
again when the song is opened, even after a restart. The song is known by a stable id saved in
the song file (older files get one from their location when opened, and keep it from their next
save). Opening another song stops a running request, saves this song's conversation and shows
the other song's.

- The panel's title is the open conversation's. Click it for the **conversations menu**: the
  song's conversations, newest first, then **New conversation**, **Rename conversation…**,
  **Delete conversation…** and **Project memory…**. The **+** in the header (or **Agent › New
  Agent Conversation**) starts a new one and keeps the old one. A new conversation is titled
  from its first request (60 characters).
- **Project memory** is what the agent should always know about the song (its key, the style,
  what to leave alone), up to 32 KB. It goes ahead of every request, for every service, as
  "Project memory (user-maintained context): … Current request: …". It is yours: agents can read
  it (`agent.memory`) but not change it, as they cannot change the standing instructions.
- A conversation reopened from the file comes back finished: a request that was running when
  ryolune quit shows as stopped, and its steps no longer link to Changes, so nothing from an
  earlier session can undo an unrelated edit. Changes itself lists the current session's edits.
- If the file cannot be read, ryolune copies it aside (`agent-conversations.unreadable-<time>.json`),
  says so under the header and stops saving conversations rather than overwrite it.

## Steering

While the agent works, the message box steers it: **Send** becomes **Steer**, with **Stop** beside
it. What you type joins the conversation at once and reaches the agent at its next step, after
the tool calls under way, as "Steering from the person… keep the work already done": the work is
not thrown away. How each service takes it:

- **API services** (Anthropic, OpenAI and the other keys, local and compatible servers): the
  steering is added after the tool results of the step in progress; if the agent was already
  writing its answer, it gets one more round for it.
- **Codex** steers the running turn (`turn/steer`); steering that arrives as the turn ends starts
  the next turn of the same thread.
- **Claude Code** reads its whole request when it starts and keeps no session here, so ryolune
  stops the run (its edits stay in the song and in Undo) and starts it again with the request,
  what it had answered so far and your steering.
- **Zenith** gets it in its thread (`thread.send`), which steers its running turn.

Steering that arrives after the agent has finished is not lost silently: the chat says it was not
read, so you can send it as a new request.

## The panel

- **Chat** shows your prompts in bubbles and streamed Markdown replies, with a short human-readable
  activity status. Technical command names, raw results and provider diagnostics live in **Changes**.
  The composer is one card: what the message is about, the text, the model picker and Send.
- **Generate** makes a loop, a song, a sound or a playable instrument from a description with the
  service in Settings > Generation, places it in one undo step, and keeps every result to audition,
  place again, turn into Sample Keys or delete. See [AI_CONTROL.md](AI_CONTROL.md#generation).
- **Changes** keeps the command history and **Undo from here / Redo to here** controls. Undo
  also removes later manual edits. These controls are disabled while the agent is working.
- The model picker groups the models reported by connected accounts and APIs, shows maker logos,
  supports search, and offers each model's advertised reasoning levels. Refreshing lists models
  without sending an inference request. A model's actual use still depends on provider access.
- Type `/` for commands, including `/diagnose` for any project problem, arrangement/mixing prompts,
  `/takes`, `/variation` and `/generate`. Commands fill a prompt or open a local feature; the prompt
  remains editable before sending.
- **Takes A/B** preserves the original before exploring a variation. Up to eight alternatives travel
  inside the saved project. Selecting a take preserves current edits, stops playback and is undoable.
- The Rhythm Lab tab was removed in 0.12; `rhythm.create` and `rhythm.preview` remain commands the
  agent, the CLI and MCP clients can call for Euclidean grooves.
- MIDI region menus also offer humanize, velocity ramps, fit-to-scale, reverse, legato and repeat.
- Enter or Cmd/Ctrl+Enter sends (steers while the agent works); Shift+Enter adds a newline. Stop
  cancels the task and keeps finished edits. The **+** in the header starts a new conversation and
  keeps the old one in the conversations menu; the project stays.
- Errors explain the next step in plain language; raw provider messages remain in Activity.

## Appearance and connection limits

**Settings > Interface** offers the one ryolune theme in **Dark**, **Light** or **Auto** (follows
the system). The change is immediate and saved; agents and the CLI can set it with `settings.set
--path interface.mode --value dark|light|auto`. `interface.appearance` is always `ryolune`.

Model discovery uses Codex app-server `model/list`, Claude Code's initialized model list, or an
API's authenticated `/models` endpoint. It does not invent future model IDs or thinking modes.
If the server does not advertise thinking capabilities, the picker uses its default. Each hosted
service keeps its own key, so switching between them never re-enters one; one other compatible
endpoint can be saved at a time. Local servers are asked for their models only while selected.
Codex currently needs file-backed CLI authentication (`cli_auth_credentials_store="file"`);
a keychain-only sign-in produces setup guidance. Third-party plugin control covers the parameters,
state, presets and native editor exposed by the host; it cannot guarantee every vendor plugin.

## From the outside

`agent.send`, `agent.steer`, `agent.stop`, `agent.status`, `agent.transcript`, `agent.providers`,
`agent.clear` and `agent.configure` drive the panel from `ryolune-cli` or MCP;
`agent.conversations`, `agent.newConversation`, `agent.selectConversation`,
`agent.renameConversation` and `agent.deleteConversation` manage the song's conversations, and
`agent.memory` / `agent.setMemory` its project memory (`agent.setMemory` and
`agent.deleteConversation` are refused to agents). `agent.mcp` lists the
configurations for outside agents. `ui.screenshot` returns a PNG of the
window so a model can see it; `ui.showPanel`, `view.set` and `ui.status` complete the picture.
