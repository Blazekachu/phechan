//! JSON persistence for vault state under `.phechan/vault/`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Vault, VaultError};

pub fn vault_path(root: &Path) -> PathBuf {
    root.join(".phechan").join("vault").join("state.json")
}

pub fn load_vault(root: &Path) -> Result<Vault, VaultError> {
    let path = vault_path(root);
    if !path.exists() {
        return Ok(Vault::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| VaultError::Message(e.to_string()))?;
    serde_json::from_str(&raw).map_err(|e| VaultError::Message(e.to_string()))
}

pub fn save_vault(root: &Path, vault: &Vault) -> Result<(), VaultError> {
    let path = vault_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| VaultError::Message(e.to_string()))?;
    }
    let raw = serde_json::to_string_pretty(vault).map_err(|e| VaultError::Message(e.to_string()))?;
    fs::write(&path, raw).map_err(|e| VaultError::Message(e.to_string()))
}
