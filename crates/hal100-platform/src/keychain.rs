use hal100_core::{SecretStore, SecretStoreError, SecretStoreOperation};

pub const DEFAULT_KEYCHAIN_SERVICE: &str = "com.hal100.desktop.backends";

#[derive(Debug, Clone)]
pub struct MacOsKeychainSecretStore {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    service: String,
}

impl Default for MacOsKeychainSecretStore {
    fn default() -> Self {
        Self::new(DEFAULT_KEYCHAIN_SERVICE)
    }
}

impl MacOsKeychainSecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for MacOsKeychainSecretStore {
    fn read(&self, credential_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        use security_framework::passwords::get_generic_password;
        use security_framework_sys::base::errSecItemNotFound;

        match get_generic_password(&self.service, credential_id) {
            Ok(secret) => Ok(Some(secret)),
            Err(error) if error.code() == errSecItemNotFound => Ok(None),
            Err(_) => Err(SecretStoreError::new(SecretStoreOperation::Read)),
        }
    }

    fn write(&self, credential_id: &str, secret: &[u8]) -> Result<(), SecretStoreError> {
        security_framework::passwords::set_generic_password(&self.service, credential_id, secret)
            .map_err(|_| SecretStoreError::new(SecretStoreOperation::Write))
    }

    fn delete(&self, credential_id: &str) -> Result<(), SecretStoreError> {
        use security_framework::passwords::delete_generic_password;
        use security_framework_sys::base::errSecItemNotFound;

        match delete_generic_password(&self.service, credential_id) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == errSecItemNotFound => Ok(()),
            Err(_) => Err(SecretStoreError::new(SecretStoreOperation::Delete)),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for MacOsKeychainSecretStore {
    fn read(&self, _credential_id: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        Err(SecretStoreError::new(SecretStoreOperation::Read))
    }

    fn write(&self, _credential_id: &str, _secret: &[u8]) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::new(SecretStoreOperation::Write))
    }

    fn delete(&self, _credential_id: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::new(SecretStoreOperation::Delete))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_a_product_scoped_keychain_service() {
        let store = MacOsKeychainSecretStore::default();
        assert_eq!(store.service, DEFAULT_KEYCHAIN_SERVICE);
        assert!(store.service.starts_with("com.hal100."));
    }
}
