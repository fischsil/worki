//! Agent secrets (API tokens) live in worki's Keychain vault, one set per isolation scope, and
//! are only read when a session starts. Status checks never read values, so they never prompt.

use worki_isolation::vault;

use crate::{Error, Result};

/// Identifies a secret's value without keeping it, such as which sign-in token a session got.
pub fn fingerprint(value: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(value.as_bytes())
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn key(scope: &str, name: &str) -> String {
    format!("env:{scope}:{name}")
}

/// Values stored before secrets were scoped.
fn legacy_key(name: &str) -> String {
    format!("env:{name}")
}

pub fn is_set(scope: &str, name: &str) -> bool {
    vault::has(&key(scope, name))
}

pub fn set(scope: &str, name: &str, value: &str) -> Result<()> {
    vault::set(&key(scope, name), value).map_err(Error::isolation)
}

pub fn delete(scope: &str, name: &str) -> Result<()> {
    vault::delete(&key(scope, name)).map_err(Error::isolation)
}

/// Secrets every agent gets.
pub const GLOBAL: &str = "global";

/// Secrets the project's agents get, over the global ones.
pub fn project_scope(project_id: &str) -> String {
    format!("project:{project_id}")
}

/// Whether `scope` holds secrets you manage rather than an isolation's sign-in.
pub fn is_managed_scope(scope: &str) -> bool {
    scope == GLOBAL || scope.starts_with("project:")
}

/// Moves `name`'s value from `from` to `to`, unless `to` already has one.
pub fn adopt(from: &str, to: &str, name: &str) -> Result<()> {
    move_value(&key(from, name), to, name)
}

/// Moves an unscoped value into `scope`, if there is one.
pub fn adopt_legacy(scope: &str, name: &str) -> Result<()> {
    move_value(&legacy_key(name), scope, name)
}

fn move_value(from: &str, to: &str, name: &str) -> Result<()> {
    if !is_set(to, name) {
        if let Some(value) = vault::get(from) {
            set(to, name, &value)?;
        }
    }
    if vault::has(from) {
        vault::delete(from).map_err(Error::isolation)?;
    }
    Ok(())
}

/// `(name, value)` pairs for the names that have a stored value in `scope`.
pub fn env(scope: &str, names: &[String]) -> Vec<(String, String)> {
    names
        .iter()
        .filter(|n| is_set(scope, n))
        .filter_map(|n| vault::get(&key(scope, n)).map(|v| (n.clone(), v)))
        .collect()
}

/// Where an account's sign-in token (`claude setup-token`) is kept.
fn account_scope(account_id: &str) -> String {
    format!("account:{account_id}")
}

pub fn has_account_token(account_id: &str, name: &str) -> bool {
    is_set(&account_scope(account_id), name)
}

pub fn account_token(account_id: &str, name: &str) -> Option<String> {
    vault::get(&key(&account_scope(account_id), name))
}

pub fn set_account_token(account_id: &str, name: &str, value: Option<&str>) -> Result<()> {
    let scope = account_scope(account_id);
    match value {
        Some(value) => set(&scope, name, value),
        None if is_set(&scope, name) => delete(&scope, name),
        None => Ok(()),
    }
}
