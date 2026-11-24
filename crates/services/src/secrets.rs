//! Passwords, tokens and keys, in the system's credential store.

use crate::ServiceError;

const SERVICE: &str = "MusicCraft";

fn entry(name: &str) -> Result<keyring::v1::Entry, ServiceError> {
    keyring::v1::Entry::new(SERVICE, name).map_err(|e| ServiceError::Secrets(e.to_string()))
}

pub fn set(name: &str, secret: &str) -> Result<(), ServiceError> {
    entry(name)?.set_password(secret).map_err(|e| ServiceError::Secrets(e.to_string()))
}

/// The secret, or nothing when none is stored.
pub fn get(name: &str) -> Option<String> {
    entry(name).ok()?.get_password().ok()
}

pub fn remove(name: &str) {
    if let Ok(e) = entry(name) {
        // Nothing to do when it was not there.
        e.delete_credential().ok();
    }
}
