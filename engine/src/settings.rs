//! User preferences shared by the window, the CLI and the MCP server. Stored as JSON in
//! the application data directory (`settings.json`, readable only by the user because it
//! may hold API keys). Every field has a default so an older or partial file still loads,
//! and every write validates the whole document first so the file is never half-broken.

use crate::{host::scan::data_dir, plugin::Format, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub const VERSION: u32 = 1;
const MASKED: &str = "••••";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub general: General,
    pub audio: Audio,
    pub interface: Interface,
    pub agent: Agent,
    pub generation: Generation,
    pub plugins: Plugins,
    pub control: Control,
    pub onboarding: Onboarding,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct General {
    pub check_updates_on_start: bool,
    pub install_updates_automatically: bool,
    /// Seconds between recovery snapshots while an edited session is idle (10-600).
    pub recovery_interval_seconds: u32,
    pub confirm_before_quit: bool,
    pub reopen_last_session: bool,
    pub last_session: Option<String>,
    pub recent_sessions: Vec<String>,
    /// Mixes and stem sets exported from the window. Counted on this computer only, so the
    /// window can ask once, after the third, whether to donate to ryolune.
    pub exports_completed: u32,
    /// The one-time support request was shown; either answer ends it for good.
    pub support_asked: bool,
    /// The version that ran last, so the window opens What's New once after an update.
    /// Absent in files written before 0.14.
    pub last_run_version: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Audio {
    pub output_device: Option<String>,
    pub input_device: Option<String>,
    pub midi_input: Option<String>,
    pub connect_midi_on_start: bool,
    /// Bars of click before a recording starts from a stopped transport, 0-4.
    pub count_in_bars: u8,
    /// Open the input while an audio track is armed so its level shows before the take.
    pub meter_input_when_armed: bool,
    /// Frames per device buffer for output and input, clamped to what the device accepts;
    /// `None` leaves the system default. Smaller is lower monitoring latency and more CPU.
    pub buffer_frames: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Interface {
    /// Theme: always `ryolune` since 0.12 (see [`THEMES`]); `mode` picks dark or light.
    pub appearance: String,
    /// `dark`, `light`, or `auto` to follow the system.
    pub mode: String,
    /// Interface zoom, 0.75-1.75.
    pub scale: f32,
    pub agent_panel_open_on_start: bool,
    pub show_tooltips: bool,
    pub follow_playhead: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// The installed Codex CLI with its own sign-in.
    Codex,
    /// The installed Claude Code CLI with its own sign-in.
    Claude,
    /// The Anthropic Messages API with an API key.
    Anthropic,
    /// The OpenAI API with an API key.
    #[serde(rename = "openai")]
    OpenAi,
    /// Google Gemini through its OpenAI-compatible endpoint, with a Gemini API key.
    Gemini,
    /// OpenRouter: hundreds of models from every lab behind one key.
    OpenRouter,
    /// Mistral's La Plateforme.
    Mistral,
    /// Groq's fast inference of open models.
    Groq,
    /// DeepSeek's API.
    DeepSeek,
    /// xAI's Grok API.
    Xai,
    /// Models running on this computer in Ollama.
    Ollama,
    /// Models running on this computer in LM Studio.
    LmStudio,
    /// Any other OpenAI-compatible endpoint (local servers, other vendors).
    Compatible,
}
/// A service that speaks the OpenAI Chat Completions API at a fixed address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hosted {
    pub base_url: &'static str,
    /// Environment variables read when the settings hold no key, in order.
    pub env: &'static [&'static str],
    /// Local servers take no key.
    pub needs_key: bool,
    /// Page where a key is created, or where the server is downloaded.
    pub help_url: &'static str,
    /// The service names its token limit `max_tokens`, not `max_completion_tokens`, and
    /// rejects fields it does not know (stream usage options).
    pub strict: bool,
}
impl Provider {
    pub const ALL: [Provider; 13] = [
        Provider::Codex,
        Provider::Claude,
        Provider::Anthropic,
        Provider::OpenAi,
        Provider::Gemini,
        Provider::OpenRouter,
        Provider::Mistral,
        Provider::Groq,
        Provider::DeepSeek,
        Provider::Xai,
        Provider::Ollama,
        Provider::LmStudio,
        Provider::Compatible,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Provider::Codex => "Codex CLI (OpenAI sign-in)",
            Provider::Claude => "Claude Code CLI (Anthropic sign-in)",
            Provider::Anthropic => "Anthropic API key",
            Provider::OpenAi => "OpenAI API key",
            Provider::Gemini => "Google Gemini API key",
            Provider::OpenRouter => "OpenRouter API key",
            Provider::Mistral => "Mistral API key",
            Provider::Groq => "Groq API key",
            Provider::DeepSeek => "DeepSeek API key",
            Provider::Xai => "xAI (Grok) API key",
            Provider::Ollama => "Ollama on this computer",
            Provider::LmStudio => "LM Studio on this computer",
            Provider::Compatible => "OpenAI-compatible endpoint",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Provider::Codex => "codex",
            Provider::Claude => "claude",
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::Gemini => "gemini",
            Provider::OpenRouter => "openrouter",
            Provider::Mistral => "mistral",
            Provider::Groq => "groq",
            Provider::DeepSeek => "deepseek",
            Provider::Xai => "xai",
            Provider::Ollama => "ollama",
            Provider::LmStudio => "lmstudio",
            Provider::Compatible => "compatible",
        }
    }
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }
    /// The installed command-line agents, which bring their own sign-in.
    pub fn is_cli(self) -> bool {
        matches!(self, Provider::Codex | Provider::Claude)
    }
    /// Every provider that runs through the OpenAI Chat Completions client: OpenAI itself,
    /// the hosted and local services and the custom endpoint.
    pub fn speaks_openai(self) -> bool {
        !matches!(
            self,
            Provider::Codex | Provider::Claude | Provider::Anthropic
        )
    }
    /// The fixed address of a hosted or local OpenAI-compatible service.
    pub fn hosted(self) -> Option<Hosted> {
        let h = |base_url, env, needs_key, help_url, strict| Hosted {
            base_url,
            env,
            needs_key,
            help_url,
            strict,
        };
        Some(match self {
            Provider::Gemini => h(
                "https://generativelanguage.googleapis.com/v1beta/openai",
                &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
                true,
                "https://aistudio.google.com/apikey",
                false,
            ),
            Provider::OpenRouter => h(
                "https://openrouter.ai/api/v1",
                &["OPENROUTER_API_KEY"],
                true,
                "https://openrouter.ai/settings/keys",
                false,
            ),
            Provider::Mistral => h(
                "https://api.mistral.ai/v1",
                &["MISTRAL_API_KEY"],
                true,
                "https://console.mistral.ai/api-keys",
                true,
            ),
            Provider::Groq => h(
                "https://api.groq.com/openai/v1",
                &["GROQ_API_KEY"],
                true,
                "https://console.groq.com/keys",
                false,
            ),
            Provider::DeepSeek => h(
                "https://api.deepseek.com/v1",
                &["DEEPSEEK_API_KEY"],
                true,
                "https://platform.deepseek.com/api_keys",
                true,
            ),
            Provider::Xai => h(
                "https://api.x.ai/v1",
                &["XAI_API_KEY"],
                true,
                "https://console.x.ai",
                false,
            ),
            Provider::Ollama => h(
                "http://127.0.0.1:11434/v1",
                &[],
                false,
                "https://ollama.com/download",
                false,
            ),
            Provider::LmStudio => h(
                "http://127.0.0.1:1234/v1",
                &[],
                false,
                "https://lmstudio.ai",
                false,
            ),
            _ => return None,
        })
    }
    /// The model used when the settings leave it blank. Only aliases a vendor keeps pointing
    /// at its current model: elsewhere the person picks one from the list the service gives.
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-sonnet-5",
            Provider::OpenAi => "gpt-5",
            Provider::OpenRouter => "openrouter/auto",
            Provider::Mistral => "mistral-large-latest",
            Provider::DeepSeek => "deepseek-chat",
            Provider::Gemini => "gemini-flash-latest",
            Provider::Codex
            | Provider::Claude
            | Provider::Groq
            | Provider::Xai
            | Provider::Ollama
            | Provider::LmStudio
            | Provider::Compatible => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Agent {
    pub provider: Provider,
    /// Model name; blank uses the provider's default.
    pub model: String,
    /// Empty delegates effort to the provider.
    pub reasoning_effort: String,
    pub anthropic_api_key: String,
    pub openai_api_key: String,
    pub gemini_api_key: String,
    pub openrouter_api_key: String,
    pub mistral_api_key: String,
    pub groq_api_key: String,
    pub deepseek_api_key: String,
    pub xai_api_key: String,
    /// Ollama's address when it is not the default `http://127.0.0.1:11434/v1`.
    pub ollama_base_url: String,
    /// LM Studio's address when it is not the default `http://127.0.0.1:1234/v1`.
    pub lmstudio_base_url: String,
    pub compatible_base_url: String,
    pub compatible_api_key: String,
    pub codex_executable: String,
    pub claude_executable: String,
    /// Largest reply the model may produce per turn.
    pub max_output_tokens: u32,
    /// Tool calls allowed in one task before the agent must answer.
    pub max_tool_rounds: u32,
    /// Extra standing instructions appended to the system prompt.
    pub instructions: String,
    pub permissions: Permissions,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Permissions {
    /// session.save/open/import/export/bounce and plugin.scan.
    pub file_operations: bool,
    /// transport.play/record/stop/locate.
    pub transport: bool,
    /// session.new and session.open replace the document.
    pub replace_session: bool,
    /// settings.set / settings.reset.
    pub settings: bool,
    /// app.quit and app.installUpdate.
    pub app_control: bool,
    /// generate.audio: sounds made through the generation service in Settings, on its credits.
    pub generation: bool,
    /// plugin.new / writeSource / build / publishLocal / install / remove / enable / disable:
    /// building and installing plugins (lsuite's PLUGINS.md). Off until the person allows it.
    pub plugins: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Plugins {
    pub scan_on_start: bool,
    pub extra_clap_paths: Vec<String>,
    pub extra_vst3_paths: Vec<String>,
    pub extra_native_paths: Vec<String>,
    /// Plugin ids starred in the browser.
    pub favorites: Vec<String>,
    /// Plugin id to the sound folder the user filed it under, overriding the automatic one.
    pub folders: std::collections::BTreeMap<String, String>,
    /// Most recently loaded plugin ids, newest first.
    pub recent: Vec<String>,
    /// Plugin ids turned off in the Plugins window (`plugin.disable`): not offered in the
    /// browser or to agents; songs that use them still play them.
    pub disabled: Vec<String>,
}
/// A service that makes audio from a description.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    /// ElevenLabs: Eleven Music for songs and loops, sound effects for one-shots.
    ElevenLabs,
    /// Stability AI's Stable Audio.
    Stability,
    /// fal.ai: any of its audio models (Stable Audio, Lyria, ACE-Step…).
    Fal,
    /// Any HTTP endpoint that follows ryolune's generation contract (docs/AI_CONTROL.md).
    Custom,
}
impl Service {
    pub const ALL: [Service; 4] = [
        Service::ElevenLabs,
        Service::Stability,
        Service::Fal,
        Service::Custom,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Service::ElevenLabs => "elevenlabs",
            Service::Stability => "stability",
            Service::Fal => "fal",
            Service::Custom => "custom",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Service::ElevenLabs => "ElevenLabs",
            Service::Stability => "Stable Audio (Stability AI)",
            Service::Fal => "fal.ai",
            Service::Custom => "Custom endpoint",
        }
    }
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
    /// Page where a key is created, or the contract a custom endpoint follows.
    pub fn help_url(self) -> &'static str {
        match self {
            Service::ElevenLabs => "https://elevenlabs.io/app/settings/api-keys",
            Service::Stability => "https://platform.stability.ai/account/keys",
            Service::Fal => "https://fal.ai/dashboard/keys",
            Service::Custom => {
                "https://github.com/ludovic111/ryolune/blob/main/docs/AI_CONTROL.md#generation"
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Generation {
    /// The service `generate.audio` uses when none is named.
    pub service: Service,
    pub elevenlabs_api_key: String,
    pub stability_api_key: String,
    pub fal_api_key: String,
    /// The fal.ai model, such as `fal-ai/stable-audio`.
    pub fal_model: String,
    pub custom_url: String,
    pub custom_api_key: String,
}
impl Default for Generation {
    fn default() -> Self {
        Self {
            service: Service::ElevenLabs,
            elevenlabs_api_key: String::new(),
            stability_api_key: String::new(),
            fal_api_key: String::new(),
            fal_model: "fal-ai/stable-audio".into(),
            custom_url: String::new(),
            custom_api_key: String::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Control {
    /// Serve the CLI/MCP bridge when the window starts.
    pub enable_bridge: bool,
}
/// The first-run setup (`app.onboarding`, `app.finishOnboarding`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Onboarding {
    /// The ryolune version the setup was finished or skipped in; empty until then, and the
    /// window shows the setup when it starts.
    pub completed: String,
    /// The app the person came from (`interop::apps` id), `none`, or empty when not asked.
    /// It decides which "bring your song" steps show first.
    pub coming_from: String,
    /// Whether they want AI features (the agent and sound generation); `None` until asked.
    /// Nothing is hidden either way yet.
    pub ai: Option<bool>,
}
impl Onboarding {
    pub fn is_done(&self) -> bool {
        !self.completed.trim().is_empty()
    }
    /// A settings file written before the setup existed belongs to someone who has used
    /// ryolune already: the setup counts as done.
    fn migrate(&mut self, stored: &str) {
        let had = serde_json::from_str::<serde_json::Value>(stored)
            .ok()
            .is_some_and(|v| v.get("onboarding").is_some());
        if !had {
            *self = Self {
                completed: "0.13".into(),
                ..Self::default()
            };
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            general: General::default(),
            audio: Audio::default(),
            interface: Interface::default(),
            agent: Agent::default(),
            generation: Generation::default(),
            plugins: Plugins::default(),
            control: Control::default(),
            onboarding: Onboarding::default(),
        }
    }
}
impl Default for General {
    fn default() -> Self {
        Self {
            check_updates_on_start: true,
            install_updates_automatically: false,
            recovery_interval_seconds: 30,
            confirm_before_quit: true,
            reopen_last_session: false,
            last_session: None,
            recent_sessions: vec![],
            exports_completed: 0,
            support_asked: false,
            last_run_version: None,
        }
    }
}
impl Default for Audio {
    fn default() -> Self {
        Self {
            output_device: None,
            input_device: None,
            midi_input: None,
            connect_midi_on_start: true,
            count_in_bars: 1,
            meter_input_when_armed: true,
            buffer_frames: None,
        }
    }
}
impl Default for Interface {
    fn default() -> Self {
        Self {
            scale: 1.0,
            appearance: THEME.into(),
            mode: "dark".into(),
            agent_panel_open_on_start: false,
            show_tooltips: true,
            follow_playhead: true,
        }
    }
}
/// The interface theme. Since 0.12 ryolune has one theme, in a dark and a light mode,
/// built in `desktop/src/ui/theme.rs` on the lsuite design system.
pub const THEME: &str = "ryolune";
/// Every accepted `interface.appearance`.
pub const THEMES: [&str; 1] = [THEME];
impl Interface {
    /// Files written before 0.6 had two appearances and no mode: `graphite` was the dark
    /// skeuomorphic look, `aero` the light glass one. From 0.6 to 0.11 there were six themes
    /// (modern, skeuo, aero, console, ink, neon), each dark or light; they all become the one
    /// theme and keep their mode.
    fn migrate(&mut self, stored: &str) {
        let had_mode = serde_json::from_str::<serde_json::Value>(stored)
            .ok()
            .is_some_and(|v| v["interface"]["mode"].is_string());
        if !had_mode {
            self.mode = if self.appearance == "aero" {
                "light"
            } else {
                "dark"
            }
            .into();
        }
        self.appearance = THEME.into();
    }
}
impl Default for Agent {
    fn default() -> Self {
        Self {
            provider: Provider::Codex,
            model: String::new(),
            reasoning_effort: String::new(),
            anthropic_api_key: String::new(),
            openai_api_key: String::new(),
            gemini_api_key: String::new(),
            openrouter_api_key: String::new(),
            mistral_api_key: String::new(),
            groq_api_key: String::new(),
            deepseek_api_key: String::new(),
            xai_api_key: String::new(),
            ollama_base_url: String::new(),
            lmstudio_base_url: String::new(),
            compatible_base_url: String::new(),
            compatible_api_key: String::new(),
            codex_executable: String::new(),
            claude_executable: String::new(),
            max_output_tokens: 4096,
            max_tool_rounds: 48,
            instructions: String::new(),
            permissions: Permissions::default(),
        }
    }
}
impl Default for Permissions {
    fn default() -> Self {
        Self {
            file_operations: true,
            transport: true,
            replace_session: false,
            settings: false,
            app_control: false,
            generation: true,
            plugins: false,
        }
    }
}
impl Default for Control {
    fn default() -> Self {
        Self {
            enable_bridge: true,
        }
    }
}

/// Where an unreadable settings file is copied before defaults replace it.
/// A file whose agent provider this version no longer offers, read with the default provider
/// instead: the rest of the settings (keys, paths, permissions) stay as they were.
fn without_retired_provider(text: &str) -> Option<Settings> {
    let mut value: Value = serde_json::from_str(text).ok()?;
    let agent = value.get_mut("agent")?.as_object_mut()?;
    let key = agent.get("provider")?.as_str()?;
    if Provider::parse(key).is_some() {
        return None;
    }
    agent.remove("provider");
    serde_json::from_value(value).ok()
}

pub fn invalid_copy(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".invalid");
    path.with_file_name(name)
}

/// Dotted paths of fields that hold secrets.
pub const SECRET_PATHS: &[&str] = &[
    "agent.anthropicApiKey",
    "agent.openaiApiKey",
    "agent.geminiApiKey",
    "agent.openrouterApiKey",
    "agent.mistralApiKey",
    "agent.groqApiKey",
    "agent.deepseekApiKey",
    "agent.xaiApiKey",
    "agent.compatibleApiKey",
    "generation.elevenlabsApiKey",
    "generation.stabilityApiKey",
    "generation.falApiKey",
    "generation.customApiKey",
];

impl Settings {
    /// `$RYOLUNE_SETTINGS`, else `settings.json` in the data directory.
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("RYOLUNE_SETTINGS").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        if let Some(sandbox) = crate::host::scan::test_sandbox() {
            return sandbox.join("settings.json");
        }
        data_dir().join("settings.json")
    }
    /// The stored settings, or defaults when the file is absent or unreadable.
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }
    /// Defaults stand in for a file that cannot be read, and the next change saves them over
    /// it: keep a copy first (`settings.json.invalid`, owner-only), so API keys and the rest
    /// survive a hand edit gone wrong or a downgrade that does not know a newer value.
    pub fn load_from(path: &Path) -> Self {
        match Self::read(path) {
            Ok(settings) => settings,
            Err(error) => {
                let backup = invalid_copy(path);
                if std::fs::copy(path, &backup).is_ok() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(
                            &backup,
                            std::fs::Permissions::from_mode(0o600),
                        );
                    }
                    crate::diagnostics::warn(&format!(
                        "{error}; kept a copy at {}",
                        backup.display()
                    ));
                } else {
                    crate::diagnostics::warn(&error);
                }
                Self::default()
            }
        }
    }
    /// The stored settings, reporting an unreadable file instead of hiding it.
    pub fn read(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                if text.len() > 4 * 1024 * 1024 {
                    return Err("Settings file exceeds 4 MiB".into());
                }
                let mut settings: Settings = serde_json::from_str(&text)
                    .or_else(|e| without_retired_provider(&text).ok_or(e))
                    .map_err(|e| format!("Invalid settings file {}: {e}", path.display()))?;
                settings.interface.migrate(&text);
                settings.onboarding.migrate(&text);
                settings.validate()?;
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
        }
    }
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path())
    }
    pub fn save_to(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut stored = self.clone();
        stored.version = VERSION;
        let text = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
        crate::document::atomic_write(path, |f| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                f.set_permissions(std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| e.to_string())?;
            }
            f.write_all(text.as_bytes()).map_err(|e| e.to_string())
        })
    }
    pub fn validate(&self) -> Result<()> {
        if !(10..=600).contains(&self.general.recovery_interval_seconds) {
            return Err("Recovery interval must be 10-600 seconds".into());
        }
        if !self.interface.scale.is_finite() || !(0.75..=1.75).contains(&self.interface.scale) {
            return Err("Interface scale must be between 0.75 and 1.75".into());
        }
        if !(256..=128_000).contains(&self.agent.max_output_tokens) {
            return Err("Agent max output tokens must be 256-128000".into());
        }
        if !(1..=500).contains(&self.agent.max_tool_rounds) {
            return Err("Agent max tool rounds must be 1-500".into());
        }
        if self.agent.instructions.len() > 20_000 {
            return Err("Agent instructions exceed 20,000 characters".into());
        }
        if self.agent.model.len() > 200 || self.agent.model.chars().any(char::is_control) {
            return Err("Model names must be printable and at most 200 characters".into());
        }
        if !THEMES.contains(&self.interface.appearance.as_str()) {
            return Err(format!(
                "Appearance must be {THEME}: there is one theme, set interface.mode to dark, light or auto"
            ));
        }
        if !["dark", "light", "auto"].contains(&self.interface.mode.as_str()) {
            return Err("Mode must be dark, light or auto".into());
        }
        if self.agent.reasoning_effort.len() > 40
            || self
                .agent
                .reasoning_effort
                .chars()
                .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        {
            return Err("Reasoning effort must be a short provider level name".into());
        }
        for (url, what) in [
            (&self.agent.compatible_base_url, "The compatible endpoint"),
            (&self.agent.ollama_base_url, "The Ollama address"),
            (&self.agent.lmstudio_base_url, "The LM Studio address"),
            (
                &self.generation.custom_url,
                "The custom generation endpoint",
            ),
        ] {
            if !(url.is_empty() || url.starts_with("http://") || url.starts_with("https://")) {
                return Err(format!("{what} must be an http(s) URL"));
            }
        }
        let fal = &self.generation.fal_model;
        if fal.len() > 200
            || !fal
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
        {
            return Err("The fal.ai model is a path such as fal-ai/stable-audio".into());
        }
        if self.general.recent_sessions.len() > 20 {
            return Err("At most 20 recent sessions are kept".into());
        }
        for key in [
            &self.agent.anthropic_api_key,
            &self.agent.openai_api_key,
            &self.agent.gemini_api_key,
            &self.agent.openrouter_api_key,
            &self.agent.mistral_api_key,
            &self.agent.groq_api_key,
            &self.agent.deepseek_api_key,
            &self.agent.xai_api_key,
            &self.agent.compatible_api_key,
            &self.generation.elevenlabs_api_key,
            &self.generation.stability_api_key,
            &self.generation.fal_api_key,
            &self.generation.custom_api_key,
        ] {
            if key.len() > 4096 || key.chars().any(char::is_control) {
                return Err("API keys must be printable and shorter than 4096 characters".into());
            }
        }
        for paths in [
            &self.plugins.extra_clap_paths,
            &self.plugins.extra_vst3_paths,
            &self.plugins.extra_native_paths,
        ] {
            if paths.len() > 64 || paths.iter().any(|p| p.is_empty() || p.len() > 4096) {
                return Err("Plugin search paths must be 1-64 non-empty entries".into());
            }
        }
        if self.onboarding.completed.len() > 40
            || self.onboarding.coming_from.len() > 40
            || self.onboarding.coming_from.chars().any(char::is_control)
        {
            return Err("Onboarding values are short names".into());
        }
        if self.audio.count_in_bars > 4 {
            return Err("Count-in is 0 to 4 bars".into());
        }
        if self
            .audio
            .buffer_frames
            .is_some_and(|frames| !(32..=4096).contains(&frames) || !frames.is_power_of_two())
        {
            return Err("Buffer size is a power of two from 32 to 4096 frames, or empty for the system default".into());
        }
        if self.plugins.favorites.len() > 4096
            || self.plugins.disabled.len() > 4096
            || self.plugins.folders.len() > 4096
            || self.plugins.recent.len() > 64
            || self.plugins.folders.values().any(|f| {
                f.trim().is_empty() || f.chars().count() > 40 || f.chars().any(char::is_control)
            })
        {
            return Err("Plugin folders need a name of 1-40 characters".into());
        }
        Ok(())
    }
    /// Extra scan directories configured for one plugin format.
    pub fn extra_plugin_paths(&self, format: Format) -> Vec<PathBuf> {
        match format {
            Format::Clap => &self.plugins.extra_clap_paths,
            Format::Vst3 => &self.plugins.extra_vst3_paths,
            Format::Native => &self.plugins.extra_native_paths,
            _ => return vec![],
        }
        .iter()
        .map(PathBuf::from)
        .collect()
    }
    /// The API key for a provider: settings first, then the conventional environment variable.
    /// A compatible endpoint is whatever server the person typed in: it gets only the key
    /// stored for it, never the OpenAI key from the environment.
    pub fn api_key(&self, provider: Provider) -> Option<String> {
        let a = &self.agent;
        let (stored, env): (&str, &[&str]) = match provider {
            Provider::Anthropic => (&a.anthropic_api_key, &["ANTHROPIC_API_KEY"]),
            Provider::OpenAi => (&a.openai_api_key, &["OPENAI_API_KEY"]),
            Provider::Gemini => (&a.gemini_api_key, provider.hosted().map_or(&[], |h| h.env)),
            Provider::OpenRouter => (&a.openrouter_api_key, &["OPENROUTER_API_KEY"]),
            Provider::Mistral => (&a.mistral_api_key, &["MISTRAL_API_KEY"]),
            Provider::Groq => (&a.groq_api_key, &["GROQ_API_KEY"]),
            Provider::DeepSeek => (&a.deepseek_api_key, &["DEEPSEEK_API_KEY"]),
            Provider::Xai => (&a.xai_api_key, &["XAI_API_KEY"]),
            Provider::Compatible => (&a.compatible_api_key, &[]),
            Provider::Codex | Provider::Claude | Provider::Ollama | Provider::LmStudio => {
                return None
            }
        };
        stored_or_env(stored, env)
    }
    /// Where an OpenAI-compatible provider listens: its fixed address, the local address the
    /// person changed, or the custom endpoint. `None` for the CLI and Anthropic providers.
    pub fn base_url(&self, provider: Provider) -> Option<String> {
        let custom = |url: &str| Some(url.trim().trim_end_matches('/').to_string());
        match provider {
            Provider::OpenAi => Some("https://api.openai.com/v1".into()),
            Provider::Compatible => custom(&self.agent.compatible_base_url),
            Provider::Ollama if !self.agent.ollama_base_url.trim().is_empty() => {
                custom(&self.agent.ollama_base_url)
            }
            Provider::LmStudio if !self.agent.lmstudio_base_url.trim().is_empty() => {
                custom(&self.agent.lmstudio_base_url)
            }
            _ => provider.hosted().map(|h| h.base_url.to_string()),
        }
    }
    /// The key for a generation service: settings first, then the conventional environment
    /// variable. A custom endpoint gets only the key stored for it.
    pub fn generation_key(&self, service: Service) -> Option<String> {
        let g = &self.generation;
        let (stored, env): (&str, &[&str]) = match service {
            Service::ElevenLabs => (&g.elevenlabs_api_key, &["ELEVENLABS_API_KEY"]),
            Service::Stability => (&g.stability_api_key, &["STABILITY_API_KEY"]),
            Service::Fal => (&g.fal_api_key, &["FAL_KEY", "FAL_API_KEY"]),
            Service::Custom => (&g.custom_api_key, &[]),
        };
        stored_or_env(stored, env)
    }
    /// Whether a generation service has what it needs to be called.
    pub fn generation_ready(&self, service: Service) -> bool {
        match service {
            Service::Custom => !self.generation.custom_url.trim().is_empty(),
            _ => self.generation_key(service).is_some(),
        }
    }
    /// The model for the configured provider.
    pub fn model(&self) -> String {
        let model = self.agent.model.trim();
        if model.is_empty() {
            self.agent.provider.default_model().to_string()
        } else {
            model.to_string()
        }
    }
    /// The whole document with secrets masked, for display and for the registry.
    pub fn redacted(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        for path in SECRET_PATHS {
            if let Some(slot) = lookup_mut(&mut value, path) {
                if let Some(text) = slot.as_str() {
                    *slot = json!(mask(text));
                }
            }
        }
        value
    }
    /// The API keys these settings hold, so the log can mask them (`diagnostics::set_secrets`).
    pub fn secrets(&self) -> Vec<String> {
        let value = serde_json::to_value(self).unwrap_or(Value::Null);
        SECRET_PATHS
            .iter()
            .filter_map(|path| lookup(&value, path)?.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
    /// Read a dotted path (`agent.model`) or the whole document when `path` is `None`.
    pub fn get(&self, path: Option<&str>) -> Result<Value> {
        let value = self.redacted();
        match path.map(str::trim).filter(|p| !p.is_empty()) {
            None => Ok(value),
            Some(path) => lookup(&value, path)
                .cloned()
                .ok_or_else(|| format!("Unknown setting `{path}`. Use settings.get to list them.")),
        }
    }
    /// Change one dotted path. Secrets set to their masked display value are left alone.
    pub fn set(&mut self, path: &str, value: Value) -> Result<()> {
        let path = path.trim();
        if path.is_empty() || path == "version" {
            return Err("Choose a setting path such as agent.model".into());
        }
        if SECRET_PATHS.contains(&path) && value.as_str().is_some_and(|s| s.starts_with(MASKED)) {
            return Ok(());
        }
        let mut document = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        let slot = lookup_mut(&mut document, path)
            .ok_or_else(|| format!("Unknown setting `{path}`. Use settings.get to list them."))?;
        if slot.is_object() {
            return Err(format!("`{path}` is a section; set one of its fields"));
        }
        *slot = coerce(slot, value)?;
        let mut next: Settings = serde_json::from_value(document)
            .map_err(|e| format!("Invalid value for `{path}`: {e}"))?;
        if next.agent.provider != self.agent.provider {
            next.agent.model.clear();
            next.agent.reasoning_effort.clear();
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    /// Reset one path (or everything) to its default.
    pub fn reset(&mut self, path: Option<&str>) -> Result<()> {
        let defaults = Settings::default();
        match path.map(str::trim).filter(|p| !p.is_empty()) {
            None => {
                *self = defaults;
                Ok(())
            }
            Some(path) => {
                let source = serde_json::to_value(&defaults).map_err(|e| e.to_string())?;
                let value = lookup(&source, path)
                    .cloned()
                    .ok_or_else(|| format!("Unknown setting `{path}`"))?;
                if value.is_object() {
                    let mut document = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
                    *lookup_mut(&mut document, path).ok_or("Unknown setting")? = value;
                    *self = serde_json::from_value(document).map_err(|e| e.to_string())?;
                    Ok(())
                } else {
                    self.set(path, value)
                }
            }
        }
    }
}

fn stored_or_env(stored: &str, env: &[&str]) -> Option<String> {
    let stored = stored.trim();
    if !stored.is_empty() {
        return Some(stored.to_string());
    }
    env.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}
fn mask(secret: &str) -> String {
    if secret.is_empty() {
        String::new()
    } else if secret.chars().count() <= 6 {
        MASKED.to_string()
    } else {
        let start = secret
            .char_indices()
            .rev()
            .nth(3)
            .map_or(0, |(index, _)| index);
        format!("{MASKED}{}", &secret[start..])
    }
}
fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |v, key| v.get(key))
}
fn lookup_mut<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    path.split('.').try_fold(value, |v, key| v.get_mut(key))
}
/// Accept strings for numbers and booleans so `settings.set` works from a shell.
fn coerce(current: &Value, value: Value) -> Result<Value> {
    Ok(match (current, &value) {
        (Value::Bool(_), Value::String(s)) => match s.as_str() {
            "true" | "on" | "yes" | "1" => json!(true),
            "false" | "off" | "no" | "0" => json!(false),
            _ => return Err("Expected true or false".into()),
        },
        (Value::Number(_), Value::String(s)) => {
            let n: f64 = s.parse().map_err(|_| "Expected a number".to_string())?;
            if n.fract() == 0.0 && current.is_u64() {
                json!(n as u64)
            } else {
                json!(n)
            }
        }
        (Value::Null, Value::String(s)) if s.is_empty() => Value::Null,
        (Value::Array(_), Value::String(s)) => {
            json!(s
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>())
        }
        _ => value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_earlier_appearance_becomes_the_one_theme_and_keeps_its_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for (stored, mode) in [
            // Before 0.6: two appearances, no mode.
            (r#"{"interface":{"appearance":"graphite"}}"#, "dark"),
            (r#"{"interface":{"appearance":"aero"}}"#, "light"),
            // 0.6 to 0.11: six themes, each with a mode.
            (
                r#"{"interface":{"appearance":"aero","mode":"dark"}}"#,
                "dark",
            ),
            (
                r#"{"interface":{"appearance":"modern","mode":"auto"}}"#,
                "auto",
            ),
            (
                r#"{"interface":{"appearance":"skeuo","mode":"light"}}"#,
                "light",
            ),
            (
                r#"{"interface":{"appearance":"console","mode":"light"}}"#,
                "light",
            ),
            (
                r#"{"interface":{"appearance":"ink","mode":"dark"}}"#,
                "dark",
            ),
            (
                r#"{"interface":{"appearance":"neon","mode":"auto"}}"#,
                "auto",
            ),
            (
                r#"{"interface":{"appearance":"ryolune","mode":"light"}}"#,
                "light",
            ),
        ] {
            std::fs::write(&path, stored).unwrap();
            let settings = Settings::read(&path).unwrap();
            assert_eq!(settings.interface.appearance, "ryolune", "{stored}");
            assert_eq!(settings.interface.mode, mode, "{stored}");
        }
        let mut settings = Settings::default();
        assert_eq!(settings.interface.appearance, "ryolune");
        assert_eq!(settings.interface.mode, "dark");
        settings
            .set("interface.appearance", json!("ryolune"))
            .unwrap();
        for old in ["graphite", "skeuo", "neon", "sepia"] {
            let err = settings
                .set("interface.appearance", json!(old))
                .unwrap_err();
            assert!(err.contains("interface.mode"), "{err}");
        }
        assert!(settings.set("interface.mode", json!("dim")).is_err());
        for mode in ["dark", "light", "auto"] {
            settings.set("interface.mode", json!(mode)).unwrap();
            assert_eq!(settings.interface.mode, mode);
        }
    }
    #[test]
    fn switching_provider_resets_an_incompatible_model_but_keeps_credentials() {
        let mut settings = Settings::default();
        settings.agent.provider = Provider::Codex;
        settings.agent.model = "a-codex-model".into();
        settings.agent.openai_api_key = "keep-this-key".into();
        settings.set("agent.provider", json!("codex")).unwrap();
        assert_eq!(settings.agent.model, "a-codex-model");
        settings.set("agent.provider", json!("openai")).unwrap();
        assert!(settings.agent.model.is_empty());
        assert_eq!(settings.agent.openai_api_key, "keep-this-key");
    }

    #[test]
    fn masked_keys_are_unicode_safe_and_do_not_expose_short_keys() {
        for key in [
            "",
            "a",
            "abcdef",
            "é😀短",
            "aé😀longsecret",
            "abcde🦀é文ß",
            "1234567",
        ] {
            let mut settings = Settings::default();
            settings.set("agent.openaiApiKey", json!(key)).unwrap();
            let shown = settings.get(Some("agent.openaiApiKey")).unwrap();
            if key.is_empty() {
                assert_eq!(shown, "");
            } else if key.chars().count() <= 6 {
                assert_eq!(shown, MASKED);
            } else {
                let masked = shown.as_str().unwrap();
                assert!(masked.starts_with(MASKED));
                assert_eq!(masked.chars().count(), 8);
                assert!(!masked.contains(key));
                assert!(key.ends_with(masked.strip_prefix(MASKED).unwrap()));
            }
            settings.set("agent.openaiApiKey", shown).unwrap();
            assert_eq!(settings.agent.openai_api_key, key);
        }
    }
    #[test]
    fn settings_round_trip_redact_secrets_and_validate_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let mut settings = Settings::default();
        settings.set("agent.provider", json!("anthropic")).unwrap();
        settings
            .set("agent.anthropicApiKey", json!("sk-ant-secret-1234"))
            .unwrap();
        settings.set("interface.scale", json!("1.25")).unwrap();
        settings
            .set("general.confirmBeforeQuit", json!("off"))
            .unwrap();
        settings
            .set("plugins.extraNativePaths", json!("/a, /b"))
            .unwrap();
        settings.save_to(&path).unwrap();
        let loaded = Settings::read(&path).unwrap();
        assert_eq!(loaded, settings);
        assert_eq!(loaded.agent.provider, Provider::Anthropic);
        assert_eq!(loaded.model(), "claude-sonnet-5");
        assert_eq!(
            loaded.api_key(Provider::Anthropic).unwrap(),
            "sk-ant-secret-1234"
        );
        let shown = loaded.get(Some("agent.anthropicApiKey")).unwrap();
        assert_eq!(shown, json!("••••1234"));
        assert!(!loaded.redacted().to_string().contains("secret"));
        assert_eq!(loaded.plugins.extra_native_paths, vec!["/a", "/b"]);
        let mut again = loaded.clone();
        again
            .set("agent.anthropicApiKey", json!("••••1234"))
            .unwrap();
        assert_eq!(again.agent.anthropic_api_key, "sk-ant-secret-1234");
        assert!(again.set("agent.nonsense", json!(1)).is_err());
        assert!(again.set("interface.scale", json!(9)).is_err());
        assert!(again.set("agent", json!({})).is_err());
        assert_eq!(again.interface.scale, 1.25);
        again.reset(Some("interface")).unwrap();
        assert_eq!(again.interface.scale, 1.0);
        again.reset(None).unwrap();
        assert_eq!(again, Settings::default());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(Settings::read(&dir.path().join("absent.json")).unwrap() == Settings::default());
        std::fs::write(&path, "{broken").unwrap();
        assert!(Settings::read(&path).is_err());
    }

    #[test]
    fn a_compatible_endpoint_never_receives_the_openai_key_from_the_environment() {
        let settings = Settings::default();
        // A compatible server without a stored key gets no key at all, even with one in the
        // environment. (Only this test reads OPENAI_API_KEY in this crate.)
        std::env::set_var("OPENAI_API_KEY", "sk-from-the-environment");
        assert_eq!(settings.api_key(Provider::Compatible), None);
        assert_eq!(
            settings.api_key(Provider::OpenAi).as_deref(),
            Some("sk-from-the-environment")
        );
        std::env::remove_var("OPENAI_API_KEY");
        let mut stored = Settings::default();
        stored.agent.compatible_api_key = " local-key ".into();
        assert_eq!(
            stored.api_key(Provider::Compatible).as_deref(),
            Some("local-key")
        );
    }

    #[test]
    fn an_unreadable_file_is_kept_before_defaults_replace_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings.agent.anthropic_api_key = "sk-ant-keep-me".into();
        settings.save_to(&path).unwrap();
        // A value this version does not accept makes the whole file unreadable.
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"scale\": 1.0", "\"scale\": 9.0");
        std::fs::write(&path, &text).unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        let kept = std::fs::read_to_string(invalid_copy(&path)).unwrap();
        assert!(kept.contains("sk-ant-keep-me"));
        // Saving the defaults afterwards leaves the copy alone.
        Settings::default().save_to(&path).unwrap();
        assert!(std::fs::read_to_string(invalid_copy(&path))
            .unwrap()
            .contains("sk-ant-keep-me"));
        // An absent file is not an error and leaves nothing behind.
        let absent = dir.path().join("absent.json");
        assert_eq!(Settings::load_from(&absent), Settings::default());
        assert!(!invalid_copy(&absent).exists());
    }

    #[test]
    fn a_provider_this_version_no_longer_offers_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings.agent.provider = Provider::Codex;
        settings.agent.anthropic_api_key = "sk-ant-keep-me".into();
        settings.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"provider\": \"codex\"", "\"provider\": \"retired\"")
            .replace(
                "\"model\": \"\"",
                "\"model\": \"\", \"retiredExecutable\": \"/opt/x\"",
            );
        std::fs::write(&path, &text).unwrap();
        let loaded = Settings::read(&path).unwrap();
        assert_eq!(loaded.agent.provider, Settings::default().agent.provider);
        assert_eq!(loaded.agent.anthropic_api_key, "sk-ant-keep-me");
        assert!(!invalid_copy(&path).exists());
        // Any other unreadable value still makes the file unreadable.
        std::fs::write(&path, text.replace("\"retired\"", "7")).unwrap();
        assert!(Settings::read(&path).is_err());
    }

    #[test]
    fn settings_saved_with_lsuite_ai_still_load() {
        // Up to 0.16, lsuite AI was a provider and Claude Code could run on it: such a file
        // loads with the default provider and keeps everything else.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings.agent.anthropic_api_key = "sk-ant-keep-me".into();
        settings.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"provider\": \"codex\"", "\"provider\": \"lsuite\"")
            .replace(
                "\"model\": \"\"",
                "\"model\": \"\", \"claudeThroughLsuite\": true",
            );
        assert!(text.contains("\"lsuite\"") && text.contains("claudeThroughLsuite"));
        std::fs::write(&path, &text).unwrap();
        let loaded = Settings::read(&path).unwrap();
        assert_eq!(loaded.agent.provider, Provider::Codex);
        assert_eq!(loaded.agent.anthropic_api_key, "sk-ant-keep-me");
        assert!(!invalid_copy(&path).exists());
    }
}
