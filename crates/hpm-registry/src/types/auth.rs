//! Authentication types and utilities

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub token: String,
    pub user_id: String,
    pub scopes: HashSet<TokenScope>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenScope {
    /// Read access to public and private packages
    Read,
    /// Publish new packages and update existing ones
    Publish,
    /// Delete packages and versions
    Delete,
    /// Administrative access
    Admin,
}

impl TokenScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            TokenScope::Read => "read",
            TokenScope::Publish => "publish",
            TokenScope::Delete => "delete",
            TokenScope::Admin => "admin",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "read" => Some(TokenScope::Read),
            "publish" => Some(TokenScope::Publish),
            "delete" => Some(TokenScope::Delete),
            "admin" => Some(TokenScope::Admin),
            _ => None,
        }
    }
}

impl AuthToken {
    pub fn new(user_id: String, scopes: HashSet<TokenScope>) -> Self {
        Self {
            token: generate_token(),
            user_id,
            scopes,
            expires_at: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_expiry(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    pub fn has_scope(&self, scope: &TokenScope) -> bool {
        self.scopes.contains(scope)
    }

    pub fn has_any_scope(&self, scopes: &[TokenScope]) -> bool {
        scopes.iter().any(|scope| self.scopes.contains(scope))
    }
}

fn generate_token() -> String {
    use ring::rand::{SecureRandom, SystemRandom};

    let rng = SystemRandom::new();
    let mut token_bytes = [0u8; 32];
    rng.fill(&mut token_bytes)
        .expect("Failed to generate random token");

    let token_b64 = base64::encode(&token_bytes);
    format!("hpm_{}", token_b64.trim_end_matches('='))
}

/// Token prefix for different token types
pub enum TokenType {
    PersonalAccess,
    Organization,
    CiCd,
}

impl TokenType {
    pub fn prefix(&self) -> &'static str {
        match self {
            TokenType::PersonalAccess => "pat",
            TokenType::Organization => "org",
            TokenType::CiCd => "ci",
        }
    }
}

// Add base64 dependency to Cargo.toml when implementing
mod base64 {
    pub fn encode(data: &[u8]) -> String {
        // Simplified base64 implementation for now
        // In production, use the `base64` crate
        hex::encode(data)
    }
}
