//! Authentication service for the registry server

use crate::types::{AuthToken, RegistryError, TokenScope};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Status};

pub struct AuthService {
    tokens: Arc<RwLock<HashMap<String, AuthToken>>>,
}

impl AuthService {
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_token(&self, token: AuthToken) {
        let mut tokens = self.tokens.write().await;
        tokens.insert(token.token.clone(), token);
    }

    pub async fn validate_token(&self, token_str: &str) -> Result<AuthToken, RegistryError> {
        let tokens = self.tokens.read().await;

        if let Some(token) = tokens.get(token_str) {
            if token.is_expired() {
                return Err(RegistryError::AuthenticationFailed(
                    "Token has expired".to_string(),
                ));
            }
            Ok(token.clone())
        } else {
            Err(RegistryError::AuthenticationFailed(
                "Invalid token".to_string(),
            ))
        }
    }

    pub async fn check_permission(
        &self,
        token: &AuthToken,
        required_scope: TokenScope,
    ) -> Result<(), RegistryError> {
        if token.has_scope(&required_scope) || token.has_scope(&TokenScope::Admin) {
            Ok(())
        } else {
            Err(RegistryError::InsufficientPermissions {
                required: required_scope.as_str().to_string(),
            })
        }
    }

    pub fn extract_token_from_request<T>(
        &self,
        request: &Request<T>,
    ) -> Result<String, RegistryError> {
        let auth_header = request.metadata().get("authorization").ok_or_else(|| {
            RegistryError::AuthenticationFailed("Missing authorization header".to_string())
        })?;

        let auth_str = auth_header.to_str().map_err(|_| {
            RegistryError::AuthenticationFailed("Invalid authorization header format".to_string())
        })?;

        if let Some(token) = auth_str.strip_prefix("Bearer ") {
            Ok(token.to_string())
        } else {
            Err(RegistryError::AuthenticationFailed(
                "Authorization header must use Bearer token".to_string(),
            ))
        }
    }

    pub async fn authenticate_request<T>(
        &self,
        request: &Request<T>,
        required_scope: TokenScope,
    ) -> Result<AuthToken, Status> {
        let token_str = self
            .extract_token_from_request(request)
            .map_err(|e| Status::unauthenticated(e.to_string()))?;

        let token = self
            .validate_token(&token_str)
            .await
            .map_err(|e| Status::unauthenticated(e.to_string()))?;

        self.check_permission(&token, required_scope)
            .await
            .map_err(|e| Status::permission_denied(e.to_string()))?;

        Ok(token)
    }
}

impl Default for AuthService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use std::collections::HashSet;

    #[tokio::test]
    async fn test_token_validation() {
        let auth_service = AuthService::new();

        let mut scopes = HashSet::new();
        scopes.insert(TokenScope::Read);

        let token = AuthToken::new("user123".to_string(), scopes);
        let token_str = token.token.clone();

        auth_service.add_token(token).await;

        let validated = auth_service.validate_token(&token_str).await;
        assert!(validated.is_ok());

        let invalid = auth_service.validate_token("invalid_token").await;
        assert!(invalid.is_err());
    }

    #[tokio::test]
    async fn test_expired_token() {
        let auth_service = AuthService::new();

        let mut scopes = HashSet::new();
        scopes.insert(TokenScope::Read);

        let token = AuthToken::new("user123".to_string(), scopes)
            .with_expiry(Utc::now() - Duration::hours(1)); // Expired 1 hour ago
        let token_str = token.token.clone();

        auth_service.add_token(token).await;

        let result = auth_service.validate_token(&token_str).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_permission_check() {
        let auth_service = AuthService::new();

        let mut read_scopes = HashSet::new();
        read_scopes.insert(TokenScope::Read);
        let read_token = AuthToken::new("user1".to_string(), read_scopes);

        let mut admin_scopes = HashSet::new();
        admin_scopes.insert(TokenScope::Admin);
        let admin_token = AuthToken::new("user2".to_string(), admin_scopes);

        // Read token should have read permission but not publish
        assert!(auth_service
            .check_permission(&read_token, TokenScope::Read)
            .await
            .is_ok());
        assert!(auth_service
            .check_permission(&read_token, TokenScope::Publish)
            .await
            .is_err());

        // Admin token should have all permissions
        assert!(auth_service
            .check_permission(&admin_token, TokenScope::Read)
            .await
            .is_ok());
        assert!(auth_service
            .check_permission(&admin_token, TokenScope::Publish)
            .await
            .is_ok());
    }
}
