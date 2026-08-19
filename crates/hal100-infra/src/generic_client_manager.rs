use std::sync::Arc;

use hal100_protocol::{GenericClientCatalog, GenericClientCredential, GenericClientSummary};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    ClientCredentialError, CredentialRegistry, Database, DatabaseError, stored_client_credential,
};

const MAX_DISPLAY_NAME_CHARS: usize = 80;

#[derive(Debug, Error)]
pub enum GenericClientManagerError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Credential(#[from] ClientCredentialError),
    #[error("客户端名称必须包含1至80个可见字符")]
    InvalidDisplayName,
    #[error("通用客户端凭据不存在")]
    ClientNotFound,
    #[error("通用客户端凭据回滚失败")]
    RollbackFailed,
}

pub struct GenericClientManager {
    database: Arc<Database>,
    credentials: CredentialRegistry,
    gateway_base_url: String,
    mutations: AsyncMutex<()>,
}

impl GenericClientManager {
    pub fn new(database: Arc<Database>, credentials: CredentialRegistry) -> Self {
        Self::with_gateway_base_url(
            database,
            credentials,
            "http://127.0.0.1:10100/v1".to_owned(),
        )
    }

    pub fn with_gateway_base_url(
        database: Arc<Database>,
        credentials: CredentialRegistry,
        gateway_base_url: String,
    ) -> Self {
        Self {
            database,
            credentials,
            gateway_base_url,
            mutations: AsyncMutex::new(()),
        }
    }

    pub fn catalog(&self) -> Result<GenericClientCatalog, GenericClientManagerError> {
        Ok(GenericClientCatalog {
            gateway_base_url: self.gateway_base_url.clone(),
            clients: self.database.generic_clients()?,
        })
    }

    pub async fn create(
        &self,
        display_name: &str,
    ) -> Result<GenericClientCredential, GenericClientManagerError> {
        let _guard = self.mutations.lock().await;
        let display_name = display_name.trim();
        if display_name.is_empty()
            || display_name.chars().count() > MAX_DISPLAY_NAME_CHARS
            || display_name.chars().any(char::is_control)
        {
            return Err(GenericClientManagerError::InvalidDisplayName);
        }
        let operation_id = Uuid::new_v4().simple().to_string();
        let client_app_id = format!("generic-{operation_id}");
        let key_id = format!("{client_app_id}-key");
        let api_key = format!(
            "hal100_client_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let credential = stored_client_credential(
            key_id,
            client_app_id.clone(),
            display_name.to_owned(),
            &api_key,
        )?;
        self.credentials.upsert(credential.clone())?;
        let now_ms = now_ms();
        if let Err(error) = self
            .database
            .insert_generic_client_credential(&credential, now_ms)
        {
            if self.credentials.remove_client(&client_app_id).is_err() {
                return Err(GenericClientManagerError::RollbackFailed);
            }
            return Err(error.into());
        }
        Ok(GenericClientCredential {
            client: GenericClientSummary {
                client_app_id,
                display_name: display_name.to_owned(),
                display_prefix: credential.display_prefix,
                created_at_ms: now_ms,
            },
            api_key,
        })
    }

    pub async fn revoke(
        &self,
        client_app_id: &str,
    ) -> Result<GenericClientCatalog, GenericClientManagerError> {
        let _guard = self.mutations.lock().await;
        if !client_app_id.starts_with("generic-") {
            return Err(GenericClientManagerError::ClientNotFound);
        }
        let summary = self
            .database
            .generic_clients()?
            .into_iter()
            .find(|client| client.client_app_id == client_app_id)
            .ok_or(GenericClientManagerError::ClientNotFound)?;
        let credential = self
            .database
            .load_client_credentials()?
            .into_iter()
            .find(|credential| credential.client_app_id == client_app_id)
            .ok_or(GenericClientManagerError::ClientNotFound)?;
        self.credentials.remove_client(client_app_id)?;
        match self
            .database
            .revoke_generic_client(client_app_id, &summary.display_name, now_ms())
        {
            Ok(true) => self.catalog(),
            Ok(false) => {
                self.credentials.upsert(credential)?;
                Err(GenericClientManagerError::ClientNotFound)
            }
            Err(error) => {
                if self.credentials.upsert(credential).is_err() {
                    return Err(GenericClientManagerError::RollbackFailed);
                }
                Err(error.into())
            }
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CredentialRegistry;

    #[tokio::test]
    async fn creates_one_time_plaintext_and_revokes_the_runtime_hash() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let credentials = CredentialRegistry::new(Vec::new());
        let gateway_credentials = credentials.clone();
        let manager = GenericClientManager::new(database.clone(), credentials);

        let created = manager.create("我的编辑器").await.expect("create client");
        assert_eq!(manager.catalog().expect("catalog").clients.len(), 1);
        assert!(gateway_credentials.authenticate(&created.api_key).is_some());
        assert!(
            !format!("{:?}", database.load_client_credentials().expect("hashes"))
                .contains(&created.api_key)
        );

        manager
            .revoke(&created.client.client_app_id)
            .await
            .expect("revoke client");
        assert!(gateway_credentials.authenticate(&created.api_key).is_none());
        assert!(manager.catalog().expect("catalog").clients.is_empty());
    }

    #[test]
    fn reports_the_gateway_address_owned_by_the_desktop_runtime() {
        let manager = GenericClientManager::with_gateway_base_url(
            Arc::new(Database::open_in_memory().expect("database")),
            CredentialRegistry::new(Vec::new()),
            "http://127.0.0.1:18432/v1".to_owned(),
        );

        assert_eq!(
            manager.catalog().expect("catalog").gateway_base_url,
            "http://127.0.0.1:18432/v1"
        );
    }
}
