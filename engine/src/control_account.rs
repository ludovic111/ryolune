//! The `account.*` commands: lsuite AI, the subscription that makes the agent work without
//! setup (lsuite's AI.md). The account itself is `account.rs`; these run anywhere (the CLI
//! and MCP in their own process, the window on a worker, since each one may wait on the
//! network). Signing in through the browser needs the window; a key works everywhere.

use crate::{
    account,
    control::{edit, opt, query, Args, Kind, Spec},
    settings, Result,
};
use serde_json::{json, Value};

pub const SPECS: &[Spec] = &[
    query("account.status", "The lsuite account this computer is signed in to (shared by every lsuite app): email, plan, the allowance used this month (`summary` reads like \"Pro · 38 % used · resets 1 Nov\"), the plan's models and where to manage it. Asks the lsuite server unless check is false. Never shows the token.", &[
        opt("check", Kind::Boolean, "Ask the server for the plan and allowance (default true); false reads only the account file."),
    ]),
    edit("account.signIn", "Sign in to lsuite AI so the agent works without any other setup, for every lsuite app on this computer. Without key, the window opens the browser to sign in (or create the account and pick a plan) and waits for it; with key, uses the key shown on the account page (lsk_…), which also works from ryolune-cli. Only a person can do this.", &[
        opt("key", Kind::String, "The key from your lsuite account page (lsk_…), for the CLI and headless use."),
    ]),
    edit("account.signOut", "Sign out of lsuite AI on this computer (every lsuite app): the server forgets the token and the account file is removed. Only a person can do this.", &[]),
    query("account.plans", "The lsuite AI plans as the server offers them: prices, models and monthly allowances (a demo for now: no payment is taken).", &[]),
    edit("account.manage", "Open the lsuite account page (plan, allowance, key) in the web browser; headless, returns its address. Only a person can do this.", &[]),
];

pub fn serves(name: &str) -> bool {
    SPECS.iter().any(|s| s.name == name)
}

/// Signing in or out and the account page are the person's.
pub fn denied_for_agent(name: &str, _permissions: &settings::Permissions) -> Option<String> {
    matches!(name, "account.signIn" | "account.signOut" | "account.manage").then(|| {
        format!("{name} is not available to agents: only the person signs in to lsuite AI, signs out or manages the plan.")
    })
}

/// Run an account command. `browser` signs in through the browser (the window passes one;
/// headless hosts cannot wait for a browser and ask for a key instead).
pub fn run(
    name: &str,
    params: &Value,
    browser: Option<&dyn Fn() -> Result<Value>>,
) -> Result<Value> {
    match name {
        "account.status" => Ok(account::status(params["check"].as_bool().unwrap_or(true))),
        "account.signIn" => match params["key"].as_str().filter(|k| !k.trim().is_empty()) {
            Some(key) => account::sign_in_with_key(key),
            None => match browser {
                Some(sign_in) => sign_in(),
                None => Err(format!(
                    "Signing in through the browser needs the ryolune window. From here, pass key=lsk_… from your account page ({}).",
                    account::manage_url()
                )),
            },
        },
        "account.signOut" => account::sign_out(),
        "account.plans" => account::plans(),
        "account.manage" => Ok(json!({ "url": account::manage_url() })),
        _ => Err(format!("Unknown account command `{name}`")),
    }
}

pub(crate) fn call(name: &str, a: &Args) -> Result<Value> {
    let params = json!({
        "key": a.opt_str("key"),
        "check": a.opt_bool("check"),
    });
    run(name, &params, None)
}
