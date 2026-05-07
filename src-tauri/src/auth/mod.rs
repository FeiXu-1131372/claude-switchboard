pub mod account_identity;
pub mod accounts;
pub mod claude_code_creds;
pub mod exchange;
pub mod keychain_guardian;
pub mod oauth_account_io;
pub mod oauth_paste_back;
pub mod orchestrator;
pub mod paths;

pub use orchestrator::AuthOrchestrator;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zeroize::ZeroizeOnDrop;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, specta::Type)]
pub enum AuthSource {
    OAuth,
    ClaudeCode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, specta::Type)]
pub struct AccountId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[zeroize(skip)]
    pub expires_at: DateTime<Utc>,
}
