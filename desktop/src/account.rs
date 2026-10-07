//! lsuite AI in the window: the `account.*` commands run on workers (each one may wait on the
//! lsuite server, a browser sign-in up to five minutes), and the last status they returned
//! is kept for Settings › Agent, first-run setup and the agent panel to show without asking
//! the server on every frame. The account file is shared with every lsuite app, so the
//! status is asked again when Settings opens and after each lsuite AI turn.

use crate::app::Ryolune;
use ryolune_engine::{account, control_account, Result};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
pub(crate) struct AccountState {
    /// What `account.status` (or a sign-in or out) last said; `None` until asked.
    pub status: Option<Value>,
    /// The account command running now, if any.
    pub busy: Option<String>,
    /// Stops a browser sign-in that is waiting.
    cancel: Option<Arc<AtomicBool>>,
    /// The last command's error, for the form that started it.
    pub error: Option<String>,
    /// The agent was working at the last tick (its turn's usage is asked for when it ends).
    pub turn_running: bool,
}

impl AccountState {
    pub fn signed_in(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s["signedIn"] == true)
            || (self.status.is_none() && account::load().is_some())
    }
    /// What the window shows: `Pro · 38 % used · resets 1 Nov`, or the email.
    pub fn summary(&self) -> String {
        self.status
            .as_ref()
            .and_then(|s| s["summary"].as_str())
            .unwrap_or("")
            .to_string()
    }
    pub fn waiting_for_browser(&self) -> bool {
        self.busy.as_deref() == Some("account.signIn") && self.cancel.is_some()
    }
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.status.as_ref().map(|s| s.to_string()).hash(&mut h);
        (&self.busy, &self.error).hash(&mut h);
        h.finish()
    }
}

/// Open a page in the person's browser (never from a test).
pub(crate) fn open_url(url: &str) -> Result<()> {
    // The address holds no secret (the state only matches this one sign-in).
    log::info!("opening {url}");
    if cfg!(test) || std::env::var_os("RYOLUNE_NO_BROWSER").is_some() {
        return Ok(());
    }
    crate::settings::reveal(std::path::Path::new(url));
    Ok(())
}

impl Ryolune {
    /// Run an `account.*` command for any client (the window, the CLI, an agent) on a
    /// worker; the reply comes when it is done.
    pub(crate) fn start_account(
        &mut self,
        method: &str,
        params: &Value,
        source: &str,
    ) -> Result<Value> {
        if method == "account.manage" {
            let url = account::manage_url();
            open_url(&url)?;
            return Ok(json!({ "url": url, "opened": true }));
        }
        let browser = method == "account.signIn"
            && params["key"].as_str().is_none_or(|k| k.trim().is_empty());
        if let Some(running) = &self.account.busy {
            if browser && running == "account.signIn" {
                return Err(
                    "A sign-in is already waiting for the browser. Finish it there, or cancel it."
                        .into(),
                );
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        if browser {
            if let Some(old) = self.account.cancel.replace(cancel.clone()) {
                old.store(true, Ordering::Release);
            }
        }
        self.account.busy = Some(method.to_string());
        self.account.error = None;
        let (method_owned, params_owned) = (method.to_string(), params.clone());
        Ok(self.start_worker(method, params, source, move || {
            let sign_in = || {
                account::sign_in_browser("ryolune", &open_url, &cancel, account::SIGN_IN_TIMEOUT)
            };
            control_account::run(&method_owned, &params_owned, Some(&sign_in))
        }))
    }

    /// Stop a browser sign-in that is still waiting.
    pub(crate) fn cancel_sign_in(&mut self) {
        if let Some(cancel) = self.account.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
    }

    /// Ask the server how the account stands, in the background (nobody waits for the
    /// answer: it lands in `account.status`). Only when signed in, so a person without an
    /// account never reaches the network for it.
    pub(crate) fn refresh_account(&mut self) {
        if self.account.busy.is_some() {
            return;
        }
        if account::load().is_none() {
            self.account.status = Some(account::status(false));
            return;
        }
        let _ = self.start_account("account.status", &json!({}), "Interface");
        // Nobody waits on it: the job answers into the cache only.
        self.attach_live = None;
    }

    /// A finished `account.*` job: keep what it says for the window.
    pub(crate) fn account_finished(&mut self, method: &str, result: &Result<Value>) {
        if self.account.busy.as_deref() == Some(method) {
            self.account.busy = None;
        }
        if method == "account.signIn" {
            self.account.cancel = None;
        }
        match result {
            Ok(value) if value.get("signedIn").is_some() => {
                self.account.status = Some(value.clone());
                if method == "account.signIn" && value["signedIn"] == true {
                    self.status = format!(
                        "Signed in to lsuite AI as {}",
                        value["email"].as_str().unwrap_or("your account")
                    );
                }
            }
            Ok(_) => {}
            Err(error) => {
                self.account.error = Some(error.clone());
                if method == "account.status" {
                    self.account.status = Some(account::status(false));
                }
            }
        }
    }
}
