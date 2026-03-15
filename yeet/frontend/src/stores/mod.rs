use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthStore {
    pub token: Option<String>,
    pub user_id: Option<Uuid>,
    pub wallet_address: Option<String>,
    pub username: Option<String>,
}

impl Default for AuthStore {
    fn default() -> Self {
        Self {
            token: None,
            user_id: None,
            wallet_address: None,
            username: None,
        }
    }
}

impl AuthStore {
    pub fn load_from_storage() -> Self {
        LocalStorage::get("yeet_auth").unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = LocalStorage::set("yeet_auth", self);
    }

    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }

    pub fn logout(&mut self) {
        *self = Self::default();
        LocalStorage::delete("yeet_auth");
    }
}
