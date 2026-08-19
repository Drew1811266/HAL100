use std::sync::{Arc, RwLock};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;

use crate::StoredClientCredential;

const MIN_CLIENT_KEY_BYTES: usize = 24;
const MAX_CLIENT_KEY_BYTES: usize = 256;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClientCredentialError {
    #[error("client credentials must contain between 24 and 256 bytes")]
    InvalidLength,
    #[error("credential identifiers and display names must be non-empty")]
    MissingIdentity,
    #[error("credential registry is unavailable")]
    RegistryUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedClient {
    pub client_app_id: String,
    pub display_name: String,
}

#[derive(Clone)]
pub struct CredentialRegistry {
    credentials: Arc<RwLock<Vec<StoredClientCredential>>>,
}

impl CredentialRegistry {
    pub fn new(credentials: Vec<StoredClientCredential>) -> Self {
        Self {
            credentials: Arc::new(RwLock::new(credentials)),
        }
    }

    pub fn authenticate(&self, plaintext_key: &str) -> Option<AuthenticatedClient> {
        if !(MIN_CLIENT_KEY_BYTES..=MAX_CLIENT_KEY_BYTES).contains(&plaintext_key.len()) {
            return None;
        }
        let candidate = hash_client_key(plaintext_key);
        let credentials = self.credentials.read().ok()?;
        credentials.iter().find_map(|credential| {
            bool::from(candidate.ct_eq(&credential.key_hash)).then(|| AuthenticatedClient {
                client_app_id: credential.client_app_id.clone(),
                display_name: credential.display_name.clone(),
            })
        })
    }

    pub fn is_empty(&self) -> bool {
        self.credentials
            .read()
            .map_or(true, |credentials| credentials.is_empty())
    }

    pub fn upsert(&self, credential: StoredClientCredential) -> Result<(), ClientCredentialError> {
        let mut credentials = self
            .credentials
            .write()
            .map_err(|_| ClientCredentialError::RegistryUnavailable)?;
        credentials.retain(|existing| existing.key_id != credential.key_id);
        credentials.push(credential);
        Ok(())
    }

    pub fn remove_client(&self, client_app_id: &str) -> Result<(), ClientCredentialError> {
        let mut credentials = self
            .credentials
            .write()
            .map_err(|_| ClientCredentialError::RegistryUnavailable)?;
        credentials.retain(|credential| credential.client_app_id != client_app_id);
        Ok(())
    }
}

pub fn hash_client_key(plaintext_key: &str) -> [u8; 32] {
    Sha256::digest(plaintext_key.as_bytes()).into()
}

pub fn stored_client_credential(
    key_id: impl Into<String>,
    client_app_id: impl Into<String>,
    display_name: impl Into<String>,
    plaintext_key: &str,
) -> Result<StoredClientCredential, ClientCredentialError> {
    let key_id = key_id.into();
    let client_app_id = client_app_id.into();
    let display_name = display_name.into();
    if key_id.trim().is_empty() || client_app_id.trim().is_empty() || display_name.trim().is_empty()
    {
        return Err(ClientCredentialError::MissingIdentity);
    }
    if !(MIN_CLIENT_KEY_BYTES..=MAX_CLIENT_KEY_BYTES).contains(&plaintext_key.len()) {
        return Err(ClientCredentialError::InvalidLength);
    }
    let visible_prefix: String = plaintext_key.chars().take(12).collect();
    let display_prefix = format!("{visible_prefix}…");

    Ok(StoredClientCredential {
        key_id,
        client_app_id,
        display_name,
        display_prefix,
        key_hash: hash_client_key(plaintext_key),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = "hal100_test_0123456789abcdef";

    #[test]
    fn authenticates_a_hash_without_retaining_plaintext() {
        let credential =
            stored_client_credential("key-1", "client-1", "Test", TEST_KEY).expect("credential");
        assert!(!format!("{credential:?}").contains(TEST_KEY));
        let registry = CredentialRegistry::new(vec![credential]);

        assert_eq!(
            registry.authenticate(TEST_KEY),
            Some(AuthenticatedClient {
                client_app_id: "client-1".to_owned(),
                display_name: "Test".to_owned(),
            })
        );
        assert!(
            registry
                .authenticate("hal100_test_wrong_wrong_wrong")
                .is_none()
        );
    }

    #[test]
    fn cloned_registry_observes_new_credentials_without_gateway_restart() {
        let registry = CredentialRegistry::new(Vec::new());
        let gateway_view = registry.clone();
        assert!(gateway_view.authenticate(TEST_KEY).is_none());

        registry
            .upsert(
                stored_client_credential("opencode-key", "opencode", "OpenCode", TEST_KEY)
                    .expect("credential"),
            )
            .expect("hot register credential");

        assert_eq!(
            gateway_view
                .authenticate(TEST_KEY)
                .expect("gateway clone sees credential")
                .client_app_id,
            "opencode"
        );

        registry
            .remove_client("opencode")
            .expect("hot revoke credential");
        assert!(gateway_view.authenticate(TEST_KEY).is_none());
    }
}
