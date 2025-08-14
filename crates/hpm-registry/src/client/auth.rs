//! Authentication management for registry client

use std::str::FromStr;
use tonic::{metadata::MetadataValue, Request};

pub struct AuthManager {
    token: Option<String>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self { token: None }
    }

    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    pub fn clear_token(&mut self) {
        self.token = None;
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    pub fn add_auth_metadata<T>(&self, mut request: Request<T>) -> Request<T> {
        if let Some(token) = &self.token {
            if let Ok(metadata_value) = MetadataValue::from_str(&format!("Bearer {}", token)) {
                request
                    .metadata_mut()
                    .insert("authorization", metadata_value);
            }
        }
        request
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Request;

    #[test]
    fn test_auth_manager() {
        let mut auth = AuthManager::new();
        assert!(!auth.has_token());

        auth.set_token("test_token".to_string());
        assert!(auth.has_token());

        let request = Request::new(());
        let request_with_auth = auth.add_auth_metadata(request);

        let auth_header = request_with_auth.metadata().get("authorization");
        assert!(auth_header.is_some());
        assert_eq!(auth_header.unwrap().to_str().unwrap(), "Bearer test_token");

        auth.clear_token();
        assert!(!auth.has_token());
    }
}
