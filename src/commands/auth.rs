//! Authentication command implementation
//!
//! Implements the AUTH command for client authentication.

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::config::SecurityConfig;
use crate::storage::MemoryStore;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// AUTH command - Authenticate to the server
///
/// Usage: AUTH password
/// Returns: OK if authentication succeeded, error otherwise
pub struct AuthCommand {
    /// Security configuration (contains requirepass)
    config: Arc<RwLock<SecurityConfig>>,
}

impl AuthCommand {
    /// Create a new AUTH command
    pub fn new(config: Arc<RwLock<SecurityConfig>>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Command for AuthCommand {
    fn name(&self) -> &'static str {
        "AUTH"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // AUTH password
    }

    async fn execute(
        &self,
        args: &[String],
        _store: &MemoryStore,
        _client_id: Uuid,
    ) -> CommandResult {
        if args.is_empty() {
            return CommandResult::Error("ERR wrong number of arguments for 'auth' command".to_string());
        }

        let password = &args[0];
        let config = self.config.read().await;

        match &config.requirepass {
            Some(required_password) => {
                if password == required_password {
                    // Password matches - success
                    // The server will mark the connection as authenticated
                    // when it sees a successful AUTH response
                    CommandResult::Ok(ResponseValue::SimpleString("OK".to_string()))
                } else {
                    // Wrong password
                    CommandResult::Error("WRONGPASS invalid username-password pair or user is disabled".to_string())
                }
            }
            None => {
                // No password configured, AUTH not required but accept it anyway
                CommandResult::Error("ERR Client sent AUTH, but no password is set".to_string())
            }
        }
    }
}

/// Commands that don't require authentication
pub const AUTH_EXEMPT_COMMANDS: &[&str] = &[
    "AUTH",
    "QUIT",
    "PING",
    "COMMAND",
    "INFO",
    "HELLO",
];

/// Check if a command is exempt from authentication
pub fn is_auth_exempt(command_name: &str) -> bool {
    AUTH_EXEMPT_COMMANDS.contains(&command_name.to_uppercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    fn test_client_id() -> Uuid {
        Uuid::new_v4()
    }

    fn create_config_with_password(password: Option<&str>) -> Arc<RwLock<SecurityConfig>> {
        Arc::new(RwLock::new(SecurityConfig {
            requirepass: password.map(|s| s.to_string()),
            notify_keyspace_events: String::new(),
        }))
    }

    #[tokio::test]
    async fn test_auth_command_correct_password() {
        let store = create_test_store();
        let config = create_config_with_password(Some("secret123"));
        let cmd = AuthCommand::new(config);
        let client_id = test_client_id();
        let args = vec!["secret123".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::SimpleString(s)) => {
                assert_eq!(s, "OK");
            }
            _ => panic!("Expected OK response"),
        }
    }

    #[tokio::test]
    async fn test_auth_command_wrong_password() {
        let store = create_test_store();
        let config = create_config_with_password(Some("secret123"));
        let cmd = AuthCommand::new(config);
        let client_id = test_client_id();
        let args = vec!["wrongpassword".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGPASS")));
    }

    #[tokio::test]
    async fn test_auth_command_no_password_configured() {
        let store = create_test_store();
        let config = create_config_with_password(None);
        let cmd = AuthCommand::new(config);
        let client_id = test_client_id();
        let args = vec!["anypassword".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("no password is set")));
    }

    #[tokio::test]
    async fn test_auth_command_no_args() {
        let store = create_test_store();
        let config = create_config_with_password(Some("secret123"));
        let cmd = AuthCommand::new(config);
        let client_id = test_client_id();
        let args: Vec<String> = vec![];

        let result = cmd.execute(&args, &store, client_id).await;

        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("wrong number")));
    }

    #[tokio::test]
    async fn test_auth_command_properties() {
        let config = create_config_with_password(Some("secret123"));
        let cmd = AuthCommand::new(config);
        assert_eq!(cmd.name(), "AUTH");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    #[test]
    fn test_is_auth_exempt() {
        // Auth-exempt commands
        assert!(is_auth_exempt("AUTH"));
        assert!(is_auth_exempt("auth"));
        assert!(is_auth_exempt("QUIT"));
        assert!(is_auth_exempt("PING"));
        assert!(is_auth_exempt("COMMAND"));
        assert!(is_auth_exempt("INFO"));
        assert!(is_auth_exempt("HELLO"));

        // Non-exempt commands
        assert!(!is_auth_exempt("GET"));
        assert!(!is_auth_exempt("SET"));
        assert!(!is_auth_exempt("DEL"));
        assert!(!is_auth_exempt("SUBSCRIBE"));
    }
}
