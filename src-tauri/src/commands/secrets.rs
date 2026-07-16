//! OpenRouter API key storage in the OS credential store (Phase 1.6).
//!
//! Windows: Credential Manager via the `keyring` crate. The key is never
//! serialized into `settings.json`. A one-way migration lifts any legacy
//! plaintext value into the store and rewrites settings without it.

use keyring::Entry;

const SERVICE: &str = "finmodel";
const ACCOUNT: &str = "openrouter_api_key";

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("credential store unavailable: {e}"))
}

/// Read the key from the OS store. `None` if missing or empty.
pub fn get_api_key() -> Option<String> {
    let entry = entry().ok()?;
    match entry.get_password() {
        Ok(p) if !p.trim().is_empty() => Some(p),
        _ => None,
    }
}

/// Write the key to the OS store. Empty string is rejected (use [`delete_api_key`]).
pub fn set_api_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("API key is empty".into());
    }
    entry()?
        .set_password(key)
        .map_err(|e| format!("failed to store API key: {e}"))
}

/// Delete the key from the OS store. Missing entry is success.
pub fn delete_api_key() -> Result<(), String> {
    let entry = match entry() {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // Already gone is fine.
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("failed to clear API key: {e}")),
    }
}

/// One-way migration: if `legacy_plaintext` is non-empty, store it in the OS
/// keyring. Returns `true` when a migration write succeeded (caller must then
/// rewrite settings without the plaintext). On keyring failure, leaves the
/// legacy value alone so the user is not locked out.
pub fn migrate_legacy_key(legacy_plaintext: &str) -> bool {
    let key = legacy_plaintext.trim();
    if key.is_empty() {
        return false;
    }
    // Prefer an existing keyring entry; only write if the store is empty.
    if get_api_key().is_some() {
        // Store already has a key — drop the legacy plaintext on rewrite.
        return true;
    }
    set_api_key(key).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_is_rejected() {
        assert!(set_api_key("").is_err());
        assert!(set_api_key("   ").is_err());
    }

    #[test]
    fn migrate_empty_is_noop() {
        assert!(!migrate_legacy_key(""));
        assert!(!migrate_legacy_key("  "));
    }
}
