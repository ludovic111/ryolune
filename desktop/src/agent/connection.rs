//! Connection checks do not send prompts or edit the session.
use ryolune_engine::settings::{Provider, Settings};
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Connection {
    provider: &'static str,
    pub(super) state: &'static str,
    message: String,
}

pub(crate) fn check(settings: &Settings) -> Connection {
    let provider = settings.agent.provider;
    let result = |state, message: &str| Connection {
        provider: provider.key(),
        state,
        message: message.into(),
    };
    if matches!(provider, Provider::Codex | Provider::Claude) && !settings.control.enable_bridge {
        return result("bridgeDisabled", "The local connection is turned off. Enable it in Settings > Control to let this account edit your project.");
    }
    let (executable, args) = match provider {
        Provider::Codex => (
            super::discover_codex(&settings.agent.codex_executable),
            vec!["login".into(), "status".into()],
        ),
        Provider::Claude => (
            super::discover_claude(&settings.agent.claude_executable),
            vec!["auth".into(), "status".into()],
        ),
        Provider::Anthropic | Provider::OpenAi => {
            return if settings.api_key(provider).is_some() {
                result(
                    "configured",
                    "API key available. Send a message to check access to the model.",
                )
            } else {
                result("missingKey", "Add an API key to connect this service.")
            };
        }
        Provider::Gemini
        | Provider::OpenRouter
        | Provider::Mistral
        | Provider::Groq
        | Provider::DeepSeek
        | Provider::Xai => {
            return if settings.api_key(provider).is_none() {
                result("missingKey", "Add an API key to connect this service.")
            } else if settings.model().is_empty() {
                result(
                    "missingModel",
                    "Key saved. Choose a model that supports tools from the list.",
                )
            } else {
                result(
                    "configured",
                    "API key available. Send a message to check access to the model.",
                )
            };
        }
        Provider::Ollama | Provider::LmStudio => {
            return if settings.model().is_empty() {
                result(
                    "missingModel",
                    "Start the app and choose one of its models that supports tools.",
                )
            } else {
                result("configured", "Model chosen. Keep the app running on this computer; send a message to check it.")
            };
        }
        Provider::Compatible => {
            return if settings.agent.compatible_base_url.trim().is_empty() {
                result(
                    "missingEndpoint",
                    "Enter your server address to connect a local or compatible model.",
                )
            } else if settings.model().is_empty() {
                result("missingModel", "Enter the model name shown in your server. Choose a model that supports tools.")
            } else {
                result("configured", "Server and model configured. Keep the server running; send a message to check access.")
            };
        }
    };
    if !executable.is_file() {
        return result(
            "missingCli",
            "Install the companion app, then check again. ryolune will look for it automatically.",
        );
    }
    if provider == Provider::Codex {
        let help = crate::settings::run_cli(
            &executable,
            &["app-server".into(), "--help".into()],
            Duration::from_secs(15),
        );
        match help {
            Ok(help) if help.contains("--listen") => {}
            Ok(_) => {
                return result(
                    "updateRequired",
                    "Update Codex to connect it to ryolune, then check again.",
                )
            }
            Err(_) => {
                return result(
                    "unavailable",
                    "Codex could not start. Check its installation or choose another service.",
                )
            }
        }
    }
    match crate::settings::run_cli(&executable, &args, Duration::from_secs(15)) {
        Ok(_) => result("signedIn", "Signed in. Your next message will use this account."),
        Err(_) => result("signInRequired", "Sign in to your account, then return to ryolune. If you are already signed in, check the installation and try again."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_bridge_blocks_cli_setup_without_changing_the_preference() {
        let mut settings = Settings::default();
        settings.agent.provider = Provider::Codex;
        settings.control.enable_bridge = false;
        assert_eq!(check(&settings).state, "bridgeDisabled");
        assert!(!settings.control.enable_bridge);
    }

    #[cfg(unix)]
    #[test]
    fn cli_connection_checks_capability_and_authentication_separately() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("codex");
        let mut settings = Settings::default();
        settings.agent.provider = Provider::Codex;
        settings.agent.codex_executable = executable.to_string_lossy().into();
        for (script, expected) in [
            ("#!/bin/sh\nprintf old-version\n", "updateRequired"),
            ("#!/bin/sh\nif [ \"$1\" = app-server ]; then printf '%s' --listen; else exit 1; fi\n", "signInRequired"),
            ("#!/bin/sh\nif [ \"$1\" = app-server ]; then printf '%s' --listen; else printf signed-in; fi\n", "signedIn"),
        ] {
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(check(&settings).state, expected);
        }
    }

    #[test]
    fn compatible_requires_both_endpoint_and_model() {
        let mut settings = Settings::default();
        settings.agent.provider = Provider::Compatible;
        assert_eq!(check(&settings).state, "missingEndpoint");
        settings.agent.compatible_base_url = "http://127.0.0.1:1234/v1".into();
        assert_eq!(check(&settings).state, "missingModel");
        settings.agent.model = "local-model".into();
        assert_eq!(check(&settings).state, "configured");
    }

    #[test]
    fn api_configuration_is_not_reported_as_verified_authentication() {
        let mut settings = Settings::default();
        settings.agent.provider = Provider::OpenAi;
        settings.agent.openai_api_key = "test-secret-do-not-expose".into();
        let connection = check(&settings);
        assert_eq!(connection.state, "configured");
        assert!(!serde_json::to_string(&connection)
            .unwrap()
            .contains("test-secret"));
    }

    #[test]
    fn missing_companion_has_an_actionable_state() {
        let mut settings = Settings::default();
        settings.agent.provider = Provider::Codex;
        let dir = tempfile::tempdir().unwrap();
        settings.agent.codex_executable = dir.path().join("absent").to_string_lossy().into();
        assert_eq!(check(&settings).state, "missingCli");
    }
}
