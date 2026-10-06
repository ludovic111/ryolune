//! Settings › Agent: choose the AI service, connect it (an account signed in through Codex or
//! Claude Code, an API key, a server on this computer or any compatible address), pick the
//! model and effort, check the connection, say what the agent may do, and connect agents
//! that live outside ryolune over MCP.
//!
//! Connection details are a draft until Save connection: nothing is written while typing,
//! the service cannot be switched over unsaved edits, and closing Settings asks first. A
//! saved key is never shown back; the field only says one exists. Every change goes through
//! `settings.set` (which refuses agents: only the person changes these).

use crate::ui::{
    daw::Daw,
    dialogs::modal::{self, request_async, Dismiss},
    theme::{radius, size, Theme},
    widgets::{
        field,
        secret_input::{secret_field, SecretInput},
        select_button, Button, InputEvent, MenuHost, MenuItem, Switch, TextInput,
    },
};
use gpui::{
    div, prelude::*, px, AnyElement, App, ClipboardItem, Context, Entity, MouseButton,
    MouseDownEvent, SharedString, Subscription, Window,
};
use ryolune_engine::settings::Provider;
use serde_json::{json, Value};

/// How a service connects, which decides the form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// An account signed in through a companion app (Codex, Claude Code).
    Account,
    /// An API key.
    Key,
    /// A server on this computer.
    Local,
    /// Any address that speaks the OpenAI chat API.
    Endpoint,
}

/// A service as Settings › Agent shows it. Mirrors `Provider` in engine/src/settings.rs (keys,
/// default models, addresses and key pages; a test keeps them in step).
pub(crate) struct ProviderInfo {
    pub provider: Provider,
    pub name: &'static str,
    pub description: &'static str,
    pub help: &'static str,
    pub destination: &'static str,
    pub kind: Kind,
    /// The settings field (under `agent.`) holding its key.
    pub key_path: Option<&'static str>,
    /// The settings field holding its address, and the address when it is blank.
    pub url_path: Option<&'static str>,
    pub default_url: Option<&'static str>,
}

const fn hosted(
    provider: Provider,
    name: &'static str,
    description: &'static str,
    help: &'static str,
    destination: &'static str,
    key_path: &'static str,
) -> ProviderInfo {
    ProviderInfo {
        provider,
        name,
        description,
        help,
        destination,
        kind: Kind::Key,
        key_path: Some(key_path),
        url_path: None,
        default_url: None,
    }
}

pub(crate) const PROVIDERS: [ProviderInfo; 14] = [
    ProviderInfo {
        provider: Provider::Codex,
        name: "Codex",
        description: "Use your OpenAI account through the Codex companion. No API key to copy.",
        help: "https://developers.openai.com/codex/cli/",
        destination: "Requests and session context are sent to OpenAI through Codex.",
        kind: Kind::Account,
        key_path: None,
        url_path: None,
        default_url: None,
    },
    ProviderInfo {
        provider: Provider::Claude,
        name: "Claude Code",
        description: "Use your Claude account through the Claude Code companion. No API key to copy.",
        help: "https://code.claude.com/docs/en/quickstart",
        destination: "Requests and session context are sent to Anthropic through Claude Code.",
        kind: Kind::Account,
        key_path: None,
        url_path: None,
        default_url: None,
    },
    hosted(
        Provider::Anthropic,
        "Anthropic API",
        "Connect with an Anthropic API key. API usage is billed separately from a chat subscription.",
        "https://console.anthropic.com/settings/keys",
        "Requests and session context are sent directly to Anthropic.",
        "anthropicApiKey",
    ),
    hosted(
        Provider::OpenAi,
        "OpenAI API",
        "Connect with an OpenAI API key. API usage is billed separately from a chat subscription.",
        "https://platform.openai.com/api-keys",
        "Requests and session context are sent directly to OpenAI.",
        "openaiApiKey",
    ),
    hosted(
        Provider::Gemini,
        "Google Gemini",
        "Connect with a Gemini API key from Google AI Studio.",
        "https://aistudio.google.com/apikey",
        "Requests and session context are sent directly to Google Gemini.",
        "geminiApiKey",
    ),
    hosted(
        Provider::OpenRouter,
        "OpenRouter",
        "One key for models from every lab: Claude, GPT, Gemini, Llama, Qwen, DeepSeek and more.",
        "https://openrouter.ai/settings/keys",
        "Requests and session context are sent directly to OpenRouter.",
        "openrouterApiKey",
    ),
    hosted(
        Provider::Mistral,
        "Mistral",
        "Connect with a Mistral API key from La Plateforme.",
        "https://console.mistral.ai/api-keys",
        "Requests and session context are sent directly to Mistral.",
        "mistralApiKey",
    ),
    hosted(
        Provider::Groq,
        "Groq",
        "Very fast open models with a Groq API key. Choose one that supports tools.",
        "https://console.groq.com/keys",
        "Requests and session context are sent directly to Groq.",
        "groqApiKey",
    ),
    hosted(
        Provider::DeepSeek,
        "DeepSeek",
        "Connect with a DeepSeek API key.",
        "https://platform.deepseek.com/api_keys",
        "Requests and session context are sent directly to DeepSeek.",
        "deepseekApiKey",
    ),
    hosted(
        Provider::Xai,
        "xAI Grok",
        "Connect with an xAI API key to use Grok.",
        "https://console.x.ai",
        "Requests and session context are sent directly to xAI Grok.",
        "xaiApiKey",
    ),
    ProviderInfo {
        provider: Provider::Ollama,
        name: "Ollama",
        description: "Models running in Ollama on this computer. Nothing leaves your machine. Choose a model that supports tools.",
        help: "https://ollama.com/download",
        destination: "Requests and session context stay on this computer, in Ollama.",
        kind: Kind::Local,
        key_path: None,
        url_path: Some("ollamaBaseUrl"),
        default_url: Some("http://127.0.0.1:11434/v1"),
    },
    ProviderInfo {
        provider: Provider::LmStudio,
        name: "LM Studio",
        description: "Models running in LM Studio on this computer. Nothing leaves your machine. Choose a model that supports tools.",
        help: "https://lmstudio.ai",
        destination: "Requests and session context stay on this computer, in LM Studio.",
        kind: Kind::Local,
        key_path: None,
        url_path: Some("lmstudioBaseUrl"),
        default_url: Some("http://127.0.0.1:1234/v1"),
    },
    ProviderInfo {
        provider: Provider::Compatible,
        name: "Other compatible server",
        description: "Any server that speaks the OpenAI chat API with tool calls, local or hosted.",
        help: "",
        destination: "Requests and session context go to the server address you choose.",
        kind: Kind::Endpoint,
        key_path: Some("compatibleApiKey"),
        url_path: Some("compatibleBaseUrl"),
        default_url: None,
    },
    ProviderInfo {
        provider: Provider::Zenith,
        name: "Zenith · lsuite",
        description: "The agents you use in zenith, lsuite's agent hub, signed in there. They work on this song through ryolune's own tools.",
        help: "https://lsuite.xyz/zenith",
        destination: "Requests and session context go to zenith and the agent you chose there.",
        kind: Kind::Account,
        key_path: None,
        url_path: None,
        default_url: None,
    },
];

/// How the service picker groups them.
pub(crate) const PROVIDER_GROUPS: [(&str, &[Provider]); 4] = [
    (
        "Your account",
        &[Provider::Codex, Provider::Claude, Provider::Zenith],
    ),
    (
        "API key",
        &[
            Provider::Anthropic,
            Provider::OpenAi,
            Provider::Gemini,
            Provider::OpenRouter,
            Provider::Mistral,
            Provider::Groq,
            Provider::DeepSeek,
            Provider::Xai,
        ],
    ),
    ("On this computer", &[Provider::Ollama, Provider::LmStudio]),
    ("Other", &[Provider::Compatible]),
];

pub(crate) fn info(provider: Provider) -> &'static ProviderInfo {
    PROVIDERS
        .iter()
        .find(|p| p.provider == provider)
        .unwrap_or(&PROVIDERS[0])
}

/// What the agent may do, in Settings words: (permission, title, description).
pub(crate) const PERMISSIONS: [(&str, &str, &str); 6] = [
    (
        "fileOperations",
        "Save, import and export files",
        "Let the agent work with files when you ask.",
    ),
    (
        "transport",
        "Play and record",
        "Let the agent control playback and recording.",
    ),
    (
        "replaceSession",
        "Replace the current project",
        "Allow opening another project or starting a new one.",
    ),
    (
        "settings",
        "Change settings",
        "Allow changes to preferences and audio devices.",
    ),
    (
        "appControl",
        "Quit and update ryolune",
        "Allow application control.",
    ),
    (
        "generation",
        "Generate sounds",
        "Let the agent make sounds with your generation service, on its credits.",
    ),
];

/// A reasoning effort as the picker names it.
pub(crate) fn effort_name(value: &str) -> String {
    match value {
        "" => "Provider default",
        "xhigh" => "Extra high",
        "ultra" => "Ultra",
        "none" => "Off",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "max" => "Max",
        other => other,
    }
    .to_string()
}

/// What an agent error means, without developer vocabulary. The agent panel says these
/// when a turn fails; they live with the service list they talk about.
#[allow(dead_code)]
pub(crate) fn agent_error_message(error: &str, unsent: bool) -> &'static str {
    let text = error.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&[
        "429",
        "rate limit",
        "ratelimit",
        "usage limit",
        "usage credits",
        "quota",
    ]) {
        "Your AI service has reached a usage limit. Check your account allowance or choose another service."
    } else if has(&[
        "401",
        "403",
        "unauthorized",
        "unauthorised",
        "invalid key",
        "invalid api key",
        "invalid token",
        "not logged in",
        "authentication",
    ]) {
        "Your AI service could not verify your account. Sign in again or check your API key in Agent settings."
    } else if has(&[
        "connection refused",
        "could not connect",
        "failed to connect",
        "error sending request",
    ]) {
        "ryolune could not reach your AI service. Check your connection and, for a local model, make sure its server is running."
    } else if unsent {
        "Your message was not sent. It is still in the box below; check the details and try again."
    } else {
        "The agent could not finish this request. Completed edits are kept in your project and in Changes."
    }
}

/// The connection as `agent.connection` reports it.
#[derive(Clone, Debug)]
struct Connection {
    state: String,
    message: String,
}

pub struct AgentForm {
    daw: Entity<Daw>,
    /// The saved provider the drafts belong to.
    provider: Provider,
    model: Entity<TextInput>,
    url: Entity<TextInput>,
    key: Entity<SecretInput>,
    executable: Entity<TextInput>,
    tokens: Entity<TextInput>,
    rounds: Entity<TextInput>,
    instructions: Entity<TextInput>,
    effort: String,
    advanced: bool,
    permissions_open: bool,
    notice: Option<String>,
    error: Option<String>,
    connection: Option<Connection>,
    checking: bool,
    /// Requests started; an answer to an older check is dropped.
    check_sequence: u64,
    models: Vec<Value>,
    mcp: Option<Value>,
    client: String,
    copied: Option<String>,
    signing_in: bool,
    menu: MenuHost,
    _subscriptions: Vec<Subscription>,
}

impl AgentForm {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = |cx: &mut Context<Self>| cx.new(TextInput::new);
        let model = text(cx);
        let url = text(cx);
        let executable = text(cx);
        let tokens = cx.new(|cx| TextInput::new(cx).mono());
        let rounds = cx.new(|cx| TextInput::new(cx).mono());
        let instructions = cx.new(|cx| {
            TextInput::new(cx)
                .multiline()
                .placeholder("Your preferred style, workflow or language…")
        });
        let key = cx.new(SecretInput::new);
        let mut subscriptions = vec![];
        for input in [&model, &url, &executable, &tokens, &rounds, &instructions] {
            subscriptions.push(cx.subscribe_in(input, window, Self::input_event));
        }
        subscriptions.push(cx.subscribe_in(&key, window, |this, _, event, window, cx| {
            this.input_event_kind(event, window, cx)
        }));
        let provider = daw.read(cx).app.settings.agent.provider;
        let mut form = Self {
            daw,
            provider,
            model,
            url,
            key,
            executable,
            tokens,
            rounds,
            instructions,
            effort: String::new(),
            advanced: false,
            permissions_open: false,
            notice: None,
            error: None,
            connection: None,
            checking: false,
            check_sequence: 0,
            models: vec![],
            mcp: None,
            client: "claude-code".into(),
            copied: None,
            signing_in: false,
            menu: MenuHost::default(),
            _subscriptions: subscriptions,
        };
        form.reset_drafts(cx);
        form
    }

    fn input_event(
        &mut self,
        _: &Entity<TextInput>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input_event_kind(event, window, cx)
    }
    fn input_event_kind(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Changed => cx.notify(),
            InputEvent::Submit => self.save_connection(cx),
            InputEvent::Cancel => window.dispatch_action(Box::new(Dismiss), cx),
            InputEvent::Blur => {}
        }
    }

    /// The saved agent settings, secrets masked.
    fn saved(&self, cx: &App) -> Value {
        self.daw.read(cx).app.settings.redacted()["agent"].clone()
    }

    /// Fill the drafts from the saved settings.
    fn reset_drafts(&mut self, cx: &mut Context<Self>) {
        let saved = self.saved(cx);
        let info = info(self.provider);
        let text = |key: &str| saved[key].as_str().unwrap_or("").to_string();
        let values = [
            (self.model.clone(), text("model")),
            (
                self.url.clone(),
                info.url_path.map(text).unwrap_or_default(),
            ),
            (
                self.executable.clone(),
                text(&format!("{}Executable", self.provider.key())),
            ),
            (self.tokens.clone(), saved["maxOutputTokens"].to_string()),
            (self.rounds.clone(), saved["maxToolRounds"].to_string()),
            (self.instructions.clone(), text("instructions")),
        ];
        for (input, value) in values {
            input.update(cx, |input, cx| input.set_text(value, cx));
        }
        self.effort = text("reasoningEffort");
        let placeholder = if info.key_path.is_some_and(|path| !text(path).is_empty()) {
            "Key saved · enter a replacement"
        } else {
            "Paste your API key"
        };
        self.key.update(cx, |key, cx| {
            key.clear(cx);
            key.set_placeholder(placeholder, cx);
        });
        let model_placeholder = if Provider::default_model(self.provider).is_empty() {
            "Model name, as the service lists it"
        } else {
            "Use the service default"
        };
        self.model
            .update(cx, |input, cx| input.set_placeholder(model_placeholder, cx));
        let url_placeholder = info.default_url.unwrap_or("http://localhost:1234/v1");
        self.url
            .update(cx, |input, cx| input.set_placeholder(url_placeholder, cx));
        self.executable.update(cx, |input, cx| {
            input.set_placeholder("Find automatically", cx)
        });
    }

    /// Connection edits not saved yet.
    pub fn unsaved(&self, cx: &App) -> bool {
        let saved = self.saved(cx);
        let info = info(self.provider);
        let text = |key: &str| saved[key].as_str().unwrap_or("").to_string();
        let number = |input: &Entity<TextInput>, key: &str| {
            input.read(cx).text().trim().parse::<u64>().ok() != saved[key].as_u64()
        };
        self.effort != text("reasoningEffort")
            || self.instructions.read(cx).text() != text("instructions")
            || number(&self.tokens, "maxOutputTokens")
            || number(&self.rounds, "maxToolRounds")
            || self.model.read(cx).text() != text("model")
            || !self.key.read(cx).is_empty()
            || info
                .url_path
                .is_some_and(|path| self.url.read(cx).text() != text(path))
            || (info.kind == Kind::Account
                && self.executable.read(cx).text()
                    != text(&format!("{}Executable", self.provider.key())))
    }

    /// Type into the key field, as the person would.
    #[cfg(test)]
    pub fn type_key(&mut self, text: &str, cx: &mut Context<Self>) {
        self.key.update(cx, |key, cx| key.insert(text, cx));
    }

    /// Discard the drafts (Settings closed over them).
    pub fn discard(&mut self, cx: &mut Context<Self>) {
        self.reset_drafts(cx);
        self.notice = None;
        self.error = None;
        cx.notify();
    }

    /// Load what the section shows: the connection, the models and the outside agents.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.check(cx);
        let daw = self.daw.clone();
        request_async(
            self,
            &daw,
            "agent.models",
            json!({}),
            cx,
            |this, result, cx| {
                this.models = result
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
                cx.notify();
            },
        );
        request_async(
            self,
            &daw,
            "agent.mcp",
            json!({}),
            cx,
            |this, result, cx| {
                match result {
                    Ok(value) if value["clients"].is_array() => this.mcp = Some(value),
                    Ok(_) => {}
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            },
        );
    }

    /// Ask `agent.connection` how the service stands. Sends nothing to a model.
    fn check(&mut self, cx: &mut Context<Self>) {
        self.check_sequence += 1;
        let sequence = self.check_sequence;
        self.checking = true;
        self.connection = None;
        cx.notify();
        let daw = self.daw.clone();
        request_async(
            self,
            &daw,
            "agent.connection",
            json!({}),
            cx,
            move |this, result, cx| {
                if sequence != this.check_sequence {
                    return;
                }
                this.checking = false;
                match result {
                    Ok(value) => {
                        this.connection = Some(Connection {
                            state: value["state"].as_str().unwrap_or("").into(),
                            message: value["message"].as_str().unwrap_or("").into(),
                        })
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            },
        );
    }

    /// Write settings under `agent.`, one by one, stopping at the first refusal.
    fn save(&mut self, entries: Vec<(String, Value)>, cx: &mut Context<Self>) -> bool {
        self.error = None;
        self.notice = None;
        let result = self.daw.update(cx, |daw, cx| {
            for (path, value) in entries {
                daw.request(
                    "settings.set",
                    json!({ "path": format!("agent.{path}"), "value": value }),
                    cx,
                )?;
            }
            Ok::<_, String>(())
        });
        let provider = self.daw.read(cx).app.settings.agent.provider;
        if provider != self.provider {
            self.provider = provider;
            self.connection = None;
        }
        match result {
            Ok(()) => {
                self.reset_drafts(cx);
                true
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                false
            }
        }
    }

    /// Save connection: every draft field, the key only when one was typed.
    fn save_connection(&mut self, cx: &mut Context<Self>) {
        if !self.unsaved(cx) || self.running(cx) {
            return;
        }
        let info = info(self.provider);
        let tokens = self.tokens.read(cx).text().trim().parse::<u64>();
        let rounds = self.rounds.read(cx).text().trim().parse::<u64>();
        let (Ok(tokens), Ok(rounds)) = (tokens, rounds) else {
            self.error = Some("The reply token and tool round limits are whole numbers.".into());
            cx.notify();
            return;
        };
        let mut entries = vec![
            (
                "model".to_string(),
                json!(self.model.read(cx).text().trim()),
            ),
            ("reasoningEffort".into(), json!(self.effort)),
            (
                "instructions".into(),
                json!(self.instructions.read(cx).text()),
            ),
            ("maxOutputTokens".into(), json!(tokens)),
            ("maxToolRounds".into(), json!(rounds)),
        ];
        if info.kind == Kind::Account {
            entries.push((
                format!("{}Executable", self.provider.key()),
                json!(self.executable.read(cx).text().trim()),
            ));
        }
        if let Some(path) = info.url_path {
            entries.push((path.into(), json!(self.url.read(cx).text().trim())));
        }
        let key = self.key.read(cx).text().trim().to_string();
        if let Some(path) = info.key_path.filter(|_| !key.is_empty()) {
            entries.push((path.into(), json!(key)));
        }
        if self.save(entries, cx) {
            self.notice = Some("Connection settings saved.".into());
            self.check(cx);
        }
    }

    fn running(&self, cx: &App) -> bool {
        self.daw.read(cx).app.agents.runtime.running()
    }

    fn sign_in(&mut self, cx: &mut Context<Self>) {
        let result = self.daw.update(cx, |daw, cx| {
            let result = daw.app.start_sign_in();
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.signing_in = true;
                self.error = None;
                self.notice = Some(
                    "Finish signing in in your browser, then return here. This can take a few minutes."
                        .into(),
                );
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    /// The sign-in job ended: say how, then check the account.
    fn follow_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.signing_in || self.daw.read(cx).app.settings_ui.job.is_some() {
            return;
        }
        self.signing_in = false;
        let failed = self
            .daw
            .update(cx, |daw, _| daw.app.settings_ui.error.take().is_some());
        if failed {
            self.notice = None;
            self.error = Some(
                "Sign-in did not finish. Try again, or follow the installation help and check your connection once signed in."
                    .into(),
            );
        } else {
            self.notice = Some("Sign-in finished. Checking your account…".into());
        }
        cx.defer_in(window, |this, _, cx| this.check(cx));
    }

    fn open_select(
        &mut self,
        items: Vec<MenuItem>,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.open(items, e.position, window, cx);
    }

    fn provider_items(&self, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let mut items = vec![];
        for (group, providers) in PROVIDER_GROUPS {
            items.push(MenuItem::Header(group.into()));
            for provider in providers {
                let provider = *provider;
                let this = cx.entity().downgrade();
                items.push(
                    MenuItem::new(info(provider).name, move |_, cx| {
                        let _ = this.update(cx, |form, cx| {
                            form.save(vec![("provider".into(), json!(provider.key()))], cx);
                            form.check(cx);
                        });
                    })
                    .checked(provider == self.provider),
                );
            }
        }
        items
    }

    fn effort_items(&self, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let model = self.model.read(cx).text().to_string();
        let offered = self
            .models
            .iter()
            .find(|g| g["provider"] == self.provider.key())
            .and_then(|g| g["models"].as_array())
            .and_then(|models| models.iter().find(|m| m["id"] == model.as_str()))
            .and_then(|m| m["efforts"].as_array())
            .map(|efforts| {
                efforts
                    .iter()
                    .filter_map(|e| e.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut choices = vec![String::new(), self.effort.clone()];
        for effort in offered {
            if !choices.contains(&effort) {
                choices.push(effort);
            }
        }
        choices.dedup();
        choices
            .into_iter()
            .map(|effort| {
                let this = cx.entity().downgrade();
                let on = effort == self.effort;
                MenuItem::new(effort_name(&effort), move |_, cx| {
                    let _ = this.update(cx, |form, cx| {
                        form.effort = effort.clone();
                        cx.notify();
                    });
                })
                .checked(on)
            })
            .collect()
    }

    fn model_items(&self) -> Vec<MenuItem> {
        let models = self
            .models
            .iter()
            .find(|g| g["provider"] == self.provider.key())
            .and_then(|g| g["models"].as_array().cloned())
            .unwrap_or_default();
        if models.is_empty() {
            return vec![
                MenuItem::new("No models listed by the service yet", |_, _| {}).disabled(true),
            ];
        }
        models
            .into_iter()
            .map(|m| {
                let id = m["id"].as_str().unwrap_or("").to_string();
                let name = m["name"].as_str().unwrap_or(&id).to_string();
                let input = self.model.clone();
                MenuItem::new(name, move |_, cx| {
                    input.update(cx, |input, cx| {
                        input.set_text(id.clone(), cx);
                        cx.emit(InputEvent::Changed);
                    })
                })
            })
            .collect()
    }

    fn text_field(&self, input: &Entity<TextInput>, window: &Window, cx: &App) -> gpui::Div {
        field(input, input.read(cx).is_focused(window), cx)
    }

    fn connection_block(&mut self, unsaved: bool, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        let info = info(self.provider);
        let account = info.kind == Kind::Account;
        let busy = self.signing_in;
        let state = self.connection.as_ref().map(|c| c.state.as_str());
        let title = if self.checking {
            "Checking connection…"
        } else {
            match state {
                Some("signedIn") => "Account connected",
                Some("configured") => "Ready to try",
                _ => "Connect your agent",
            }
        };
        let message = self.notice.clone().or_else(|| {
            if self.checking {
                Some("Checking this computer. No message is sent to an AI model.".into())
            } else {
                self.connection.as_ref().map(|c| c.message.clone())
            }
        });
        let can_chat = matches!(state, Some("signedIn" | "configured"));
        let blocked = busy || self.checking || unsaved;
        let lit = matches!(state, Some("signedIn" | "configured")) && !self.checking;
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(14.0))
            .rounded(px(radius::MD))
            .bg(theme.bg_raised)
            .border_1()
            .border_color(if lit { theme.accent_ring } else { theme.line })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(crate::ui::widgets::dot(
                        if lit { theme.accent } else { theme.text_3 },
                        7.0,
                    ))
                    .child(
                        div()
                            .text_size(px(size::BASE))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(title),
                    ),
            )
            .when_some(message, |d, m| d.child(modal::text(m, cx)))
            .when_some(self.error.clone(), |d, e| d.child(modal::error_line(e, cx)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .when(state == Some("bridgeDisabled"), |d| {
                        d.child(
                            Button::new("agent-bridge", "Open connection settings").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.daw.update(cx, |daw, cx| {
                                        daw.run(
                                            "ui.showPanel",
                                            json!({"panel": "settings", "section": "control"}),
                                            cx,
                                        );
                                    })
                                }),
                            ),
                        )
                    })
                    .when(
                        account && matches!(state, Some("signInRequired" | "signedIn")),
                        |d| {
                            d.child(
                                Button::new(
                                    "agent-sign-in",
                                    if state == Some("signedIn") {
                                        "Sign in again".to_string()
                                    } else {
                                        format!("Sign in with {}", info.name)
                                    },
                                )
                                .disabled(blocked)
                                .on_click(cx.listener(|this, _, _, cx| this.sign_in(cx))),
                            )
                        },
                    )
                    .child(
                        Button::new("agent-check", "Check connection")
                            .disabled(blocked)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notice = None;
                                this.error = None;
                                this.check(cx)
                            })),
                    )
                    .when(can_chat, |d| {
                        d.child(
                            Button::new("agent-chat", "Start chatting")
                                .primary()
                                .disabled(blocked)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.daw.update(cx, |daw, cx| {
                                        daw.run(
                                            "ui.showPanel",
                                            json!({"panel": "settings", "visible": false}),
                                            cx,
                                        );
                                        daw.run("ui.showPanel", json!({"panel": "agent"}), cx);
                                    })
                                })),
                        )
                    }),
            )
            .when(!info.help.is_empty(), |d| {
                let help = info.help;
                d.child(
                    div().flex().child(
                        Button::new(
                            "agent-help",
                            match info.kind {
                                Kind::Account => "Installation and sign-in help".to_string(),
                                Kind::Local => format!("Download {}", info.name),
                                _ => "Get an API key".to_string(),
                            },
                        )
                        .ghost()
                        .compact()
                        .with_icon("external")
                        .on_click(move |_, _, cx| cx.open_url(help)),
                    ),
                )
            })
    }

    fn external_agents(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .pt(px(8.0))
            .child(modal::heading("Use another agent", cx))
            .child(modal::text(
                "Claude Code, Codex, Cursor and any app that speaks MCP can work on this song from outside. They get the same commands as the built-in agent, and every change they make lands in Undo.",
                cx,
            ));
        let Some(mcp) = self.mcp.clone() else {
            return section;
        };
        if mcp["bridgeEnabled"] == false {
            section = section.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.0))
                    .p(px(10.0))
                    .rounded(px(radius::SM))
                    .bg(theme.well)
                    .child(modal::text(
                        "The local connection is off, so outside agents cannot reach this window.",
                        cx,
                    ))
                    .child(Button::new("mcp-bridge", "Turn it on").compact().on_click(
                        cx.listener(|this, _, _, cx| {
                            this.daw.update(cx, |daw, cx| {
                                daw.run(
                                    "ui.showPanel",
                                    json!({"panel": "settings", "section": "control"}),
                                    cx,
                                );
                            })
                        }),
                    )),
            );
        }
        let clients = mcp["clients"].as_array().cloned().unwrap_or_default();
        let Some(client) = clients
            .iter()
            .find(|c| c["id"] == self.client.as_str())
            .or_else(|| clients.first())
            .cloned()
        else {
            return section;
        };
        let id = client["id"].as_str().unwrap_or("").to_string();
        let name = client["name"].as_str().unwrap_or("").to_string();
        let text = client["text"].as_str().unwrap_or("").to_string();
        section = section.child(div().flex().flex_wrap().gap(px(4.0)).children(
            clients.iter().enumerate().map(|(i, c)| {
                let cid = c["id"].as_str().unwrap_or("").to_string();
                Button::new(
                    ("mcp-client", i),
                    c["name"].as_str().unwrap_or("").to_string(),
                )
                .compact()
                .ghost()
                .lit(cid == id)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.client = cid.clone();
                    cx.notify();
                }))
            }),
        ));
        section = section
            .child(modal::text(
                client["how"].as_str().unwrap_or("").to_string(),
                cx,
            ))
            .when_some(client["file"].as_str().map(str::to_string), |d, file| {
                d.child(modal::note(format!("File: {file}"), cx))
            })
            .child(modal::well(text.clone(), cx));
        let short = name.split(" (").next().unwrap_or(&name).to_string();
        let copied = self.copied.as_deref() == Some(id.as_str());
        let link_id = id.clone();
        section.child(
            div()
                .flex()
                .gap(px(8.0))
                .when(client["link"] == true, |d| {
                    d.child(
                        Button::new("mcp-add", format!("Add to {short}"))
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let client = link_id.clone();
                                let result = this.daw.update(cx, |daw, cx| {
                                    daw.request("agent.openClient", json!({ "client": client }), cx)
                                });
                                if let Err(error) = result {
                                    this.error = Some(error);
                                    cx.notify();
                                }
                            })),
                    )
                })
                .child(
                    Button::new("mcp-copy", if copied { "Copied" } else { "Copy" })
                        .with_icon("copy")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            this.copied = Some(id.clone());
                            cx.notify();
                        })),
                )
                .child(modal::note(
                    "Keep ryolune open while the agent works: it edits this window.",
                    cx,
                )),
        )
    }
}

impl Render for AgentForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        self.follow_sign_in(window, cx);
        // The service changed elsewhere (the CLI, the agent panel's model picker): start over.
        let saved_provider = self.daw.read(cx).app.settings.agent.provider;
        if saved_provider != self.provider && !self.unsaved(cx) {
            self.provider = saved_provider;
            self.reset_drafts(cx);
            cx.defer_in(window, |this, _, cx| this.check(cx));
        }
        let info = info(self.provider);
        let account = info.kind == Kind::Account;
        let model_required = !account && Provider::default_model(self.provider).is_empty();
        let unsaved = self.unsaved(cx);
        let running = self.running(cx);
        let locked = unsaved || running || self.signing_in;
        let saved = self.saved(cx);
        let has_key = info
            .key_path
            .is_some_and(|p| !saved[p].as_str().unwrap_or("").is_empty());
        let key_focused = self.key.read(cx).is_focused(window);

        let provider_select = select_button("agent-provider", info.name, cx)
            .min_w(px(220.0))
            .when(locked, |d| d.opacity(0.5))
            .when(!locked, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, window, cx| {
                        let items = this.provider_items(cx);
                        this.open_select(items, e, window, cx);
                    }),
                )
            });

        let mut form = div().flex().flex_col().gap(px(12.0));
        if let Some(path) = info.url_path {
            let _ = path;
            form = form.child(modal::stacked(
                "Server address",
                Some(
                    if info.kind == Kind::Local {
                        format!("Leave empty for {}'s usual address.", info.name)
                    } else {
                        "Use the OpenAI-compatible address from your server, including /v1 when required."
                            .to_string()
                    }
                    .into(),
                ),
                self.text_field(&self.url, window, cx),
                cx,
            ));
        }
        if info.key_path.is_some() {
            form = form.child(modal::stacked(
                if info.kind == Kind::Endpoint {
                    "API key (optional)"
                } else {
                    "API key"
                },
                None,
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .child(secret_field(&self.key, key_focused, cx)),
                    )
                    .when(has_key, |d| {
                        d.child(
                            Button::new("agent-remove-key", "Remove saved key")
                                .disabled(unsaved)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(path) = info_path(this.provider) {
                                        if this.save(vec![(path.into(), json!(""))], cx) {
                                            this.notice = Some("Key removed.".into());
                                            this.check(cx);
                                        }
                                    }
                                })),
                        )
                    }),
                cx,
            ));
        }
        if model_required {
            form = form.child(modal::stacked(
                "Model name",
                Some("Choose one that supports tools.".into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .child(self.text_field(&self.model, window, cx)),
                    )
                    .child(
                        select_button("agent-models", "Your models", cx).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, e: &MouseDownEvent, window, cx| {
                                let items = this.model_items();
                                this.open_select(items, e, window, cx);
                            }),
                        ),
                    ),
                cx,
            ));
        }
        form = form.child(modal::field_row(
            "Reasoning effort",
            Some("How long the model thinks before it answers.".into()),
            select_button("agent-effort", effort_name(&self.effort), cx)
                .min_w(px(160.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, window, cx| {
                        let items = this.effort_items(cx);
                        this.open_select(items, e, window, cx);
                    }),
                ),
            cx,
        ));
        // Advanced connection settings.
        form = form.child(disclosure(
            "agent-advanced",
            "Advanced connection settings",
            self.advanced,
            cx.listener(|this, _, _, cx| {
                this.advanced = !this.advanced;
                cx.notify();
            }),
            cx,
        ));
        if self.advanced {
            let mut advanced = div().flex().flex_col().gap(px(12.0)).pl(px(14.0));
            if !model_required {
                advanced = advanced.child(modal::stacked(
                    "Model (optional)",
                    None,
                    self.text_field(&self.model, window, cx),
                    cx,
                ));
            }
            if account {
                advanced = advanced.child(modal::stacked(
                    "Companion executable (optional)",
                    None,
                    self.text_field(&self.executable, window, cx),
                    cx,
                ));
            }
            advanced = advanced
                .child(modal::field_row(
                    "Reply token limit",
                    Some("256 to 128,000.".into()),
                    div().w(px(110.0)).child(self.text_field(&self.tokens, window, cx)),
                    cx,
                ))
                .child(modal::field_row(
                    "Tool round limit",
                    Some("Tool calls in one task before the agent must answer, 1 to 500.".into()),
                    div().w(px(110.0)).child(self.text_field(&self.rounds, window, cx)),
                    cx,
                ))
                .child(modal::stacked(
                    "Standing instructions",
                    None,
                    self.text_field(&self.instructions, window, cx).min_h(px(72.0)),
                    cx,
                ))
                .child(modal::note(
                    "Leave optional fields empty to use automatic settings. Changing service resets the model to its default.",
                    cx,
                ));
            form = form.child(advanced);
        }
        if unsaved || !account {
            form = form.child(
                div().flex().child(
                    Button::new("agent-save", "Save connection")
                        .primary()
                        .disabled(!unsaved || running)
                        .on_click(cx.listener(|this, _, _, cx| this.save_connection(cx))),
                ),
            );
        }
        if unsaved {
            form = form.child(modal::note(
                "Save your connection changes before switching service or connecting.",
                cx,
            ));
        }

        let permissions = self.daw.read(cx).app.settings.agent.permissions.clone();
        let permission_value = |key: &str| match key {
            "fileOperations" => permissions.file_operations,
            "transport" => permissions.transport,
            "replaceSession" => permissions.replace_session,
            "settings" => permissions.settings,
            "appControl" => permissions.app_control,
            _ => permissions.generation,
        };
        let mut can = div().flex().flex_col();
        if self.permissions_open {
            for (key, title, description) in PERMISSIONS {
                let on = permission_value(key);
                can = can.child(modal::field_row(
                    title,
                    Some(description.into()),
                    div().when(locked, |d| d.opacity(0.4)).child(
                        Switch::new(SharedString::from(format!("perm-{key}")), on).on_toggle({
                            let set = cx.listener(move |this, on: &bool, _, cx| {
                                if !this.unsaved(cx) && !this.running(cx) {
                                    this.save(vec![(format!("permissions.{key}"), json!(on))], cx);
                                    cx.notify();
                                }
                            });
                            move |on, window, cx| set(&on, window, cx)
                        }),
                    ),
                    cx,
                ));
            }
        }

        let connection = self.connection_block(unsaved, cx);
        let external = self.external_agents(cx);
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(modal::heading("Make music with an agent", cx))
            .child(modal::text(
                "Choose a service, connect it, then describe what you want to hear.",
                cx,
            ))
            .child(modal::field_row("AI service", Some(info.description.into()), provider_select, cx))
            .when(running, |d| {
                d.child(modal::note("Stop the agent to change its connection.", cx))
            })
            .child(form)
            .child(connection)
            .child(modal::note(
                format!(
                    "{} The agent can edit this project; edits appear in Changes and can be undone.",
                    info.destination
                ),
                cx,
            ))
            .child(disclosure(
                "agent-permissions",
                "What the agent can do",
                self.permissions_open,
                cx.listener(|this, _, _, cx| {
                    this.permissions_open = !this.permissions_open;
                    cx.notify();
                }),
                cx,
            ))
            .child(can)
            .child(div().h(px(1.0)).bg(theme.hairline))
            .child(external)
            .children(self.menu.render(window, cx))
    }
}

fn info_path(provider: Provider) -> Option<&'static str> {
    info(provider).key_path
}

/// A row that opens or closes a group of settings.
pub(crate) fn disclosure(
    id: &'static str,
    label: &'static str,
    open: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = Theme::get(cx);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.0))
        .py(px(4.0))
        .cursor_pointer()
        .text_size(px(size::BASE))
        .text_color(theme.text_2)
        .hover(|s| s.text_color(theme.text))
        .child(crate::ui::widgets::icon(
            if open {
                "chevron-down"
            } else {
                "chevron-right"
            },
            9.0,
            theme.text_3,
        ))
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_engine::settings::Settings;

    #[test]
    fn the_service_list_mirrors_the_engine() {
        let defaults = Settings::default().redacted();
        let agent = &defaults["agent"];
        for provider in Provider::ALL {
            let entry = info(provider);
            assert_eq!(entry.provider, provider, "{} has an entry", provider.key());
            for path in entry.key_path.iter().chain(entry.url_path.iter()) {
                assert!(agent.get(*path).is_some(), "agent.{path} exists");
            }
            if let Some(hosted) = provider.hosted() {
                assert_eq!(entry.help, hosted.help_url, "{}", provider.key());
                assert_eq!(entry.key_path.is_some(), hosted.needs_key);
                if entry.kind == Kind::Local {
                    assert_eq!(entry.default_url, Some(hosted.base_url));
                }
            }
            assert_eq!(entry.kind == Kind::Account, provider.is_cli());
            let path = format!("agent.{}", entry.key_path.unwrap_or("model"));
            if entry.key_path.is_some() {
                assert!(
                    ryolune_engine::settings::SECRET_PATHS.contains(&path.as_str()),
                    "{path} is masked"
                );
            }
        }
        let grouped: Vec<Provider> = PROVIDER_GROUPS
            .iter()
            .flat_map(|(_, list)| list.iter().copied())
            .collect();
        assert_eq!(grouped.len(), Provider::ALL.len());
        for provider in Provider::ALL {
            assert!(grouped.contains(&provider));
        }
        for (key, _, _) in PERMISSIONS {
            assert!(agent["permissions"].get(key).is_some(), "permissions.{key}");
        }
    }

    #[test]
    fn connection_feedback_needs_no_developer_vocabulary() {
        for (reason, expected) in [
            ("HTTP 429", "usage limit"),
            ("401 Unauthorized", "verify your account"),
            ("Connection refused", "could not reach"),
        ] {
            assert!(
                agent_error_message(reason, false).contains(expected),
                "{reason}"
            );
        }
        assert!(agent_error_message("boom", true).contains("not sent"));
    }

    fn scratch_settings() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("ryolune-ui-test-{}", std::process::id()));
            std::env::set_var("RYOLUNE_SETTINGS", dir.join("settings.json"));
        });
    }

    fn form(
        cx: &mut gpui::TestAppContext,
        setup: impl FnOnce(&mut Settings),
    ) -> (Entity<Daw>, Entity<AgentForm>) {
        scratch_settings();
        let mut app = crate::app::Ryolune::from_session(ryolune_engine::store::empty(), None);
        setup(&mut app.settings);
        let daw = cx.new(|_| Daw::new(app));
        cx.update(|cx| {
            cx.set_global(Theme::new(crate::ui::theme::Mode::Dark, true));
            crate::ui::widgets::bind(cx);
        });
        let window = cx.add_window(|window, cx| AgentForm::new(daw.clone(), window, cx));
        let form = window.root(cx).unwrap();
        (daw, form)
    }

    #[gpui::test]
    fn a_saved_key_is_never_put_in_the_field(cx: &mut gpui::TestAppContext) {
        let (_, form) = form(cx, |s| {
            s.agent.provider = Provider::OpenAi;
            s.agent.openai_api_key = "sk-secret-1234".into();
        });
        cx.update(|cx| {
            let form = form.read(cx);
            assert!(form.key.read(cx).is_empty());
            assert!(form.key.read(cx).placeholder().contains("Key saved"));
            assert!(info(form.provider).url_path.is_none(), "no server address");
            assert!(!form.unsaved(cx));
        });
    }

    #[gpui::test]
    fn the_key_is_saved_only_explicitly_and_trimmed(cx: &mut gpui::TestAppContext) {
        let (daw, form) = form(cx, |s| s.agent.provider = Provider::OpenAi);
        cx.update(|cx| {
            form.update(cx, |form, cx| {
                form.key.update(cx, |key, cx| key.insert(" test-key ", cx));
            });
            assert_eq!(daw.read(cx).app.settings.agent.openai_api_key, "");
            assert!(form.read(cx).unsaved(cx), "a typed key is a draft");
            form.update(cx, |form, cx| form.save_connection(cx));
            assert_eq!(daw.read(cx).app.settings.agent.openai_api_key, "test-key");
            assert!(
                form.read(cx).key.read(cx).is_empty(),
                "the field forgets it"
            );
        });
    }

    #[gpui::test]
    fn unsaved_edits_lock_the_service_and_the_check(cx: &mut gpui::TestAppContext) {
        let (_, form) = form(cx, |s| {
            s.agent.provider = Provider::Compatible;
            s.agent.compatible_base_url = "http://localhost:1234/v1".into();
            s.agent.model = "local".into();
        });
        cx.update(|cx| {
            assert!(!form.read(cx).unsaved(cx));
            form.update(cx, |form, cx| {
                form.model
                    .update(cx, |m, cx| m.set_text("another-model", cx));
            });
            assert!(form.read(cx).unsaved(cx));
            form.update(cx, |form, cx| form.discard(cx));
            assert!(!form.read(cx).unsaved(cx));
            assert_eq!(form.read(cx).model.read(cx).text(), "local");
        });
    }
}
