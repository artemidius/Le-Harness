use crate::settings::{
    ApiKey, CredentialPrompt, InteractionPolicy, SettingsError, resolve_api_key,
};

const SERVICE: &str = "com.artemidius.le-harness";
const ACCOUNT: &str = "mistral-api-key";

/// Origin of the credential selected for a cloud operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    Environment,
    SecureStore,
    Prompt,
}

/// A resolved key plus a non-fatal secure-store warning, if persistence was unavailable.
pub struct ResolvedApiKey {
    pub api_key: ApiKey,
    pub source: CredentialSource,
    pub store_warning: Option<String>,
}

/// Minimal secret-store boundary used by the resolver and deterministic tests.
pub trait CredentialStore {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, secret: &str) -> Result<(), String>;
    fn delete(&self) -> Result<bool, String>;
}

/// Native secure storage for the current operating system.
///
/// This maps to Keychain on macOS, Credential Manager on Windows, and a
/// Secret Service-backed user keyring on Linux.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsCredentialStore;

impl OsCredentialStore {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|error| error.to_string())
    }
}

/// A no-op store for explicitly ephemeral credential handling.
#[derive(Debug, Clone, Copy, Default)]
pub struct EphemeralCredentialStore;

impl CredentialStore for EphemeralCredentialStore {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn save(&self, _secret: &str) -> Result<(), String> {
        Ok(())
    }

    fn delete(&self) -> Result<bool, String> {
        Ok(false)
    }
}

impl CredentialStore for OsCredentialStore {
    fn load(&self) -> Result<Option<String>, String> {
        match Self::entry()?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&self, secret: &str) -> Result<(), String> {
        Self::entry()?
            .set_password(secret)
            .map_err(|error| error.to_string())
    }

    fn delete(&self) -> Result<bool, String> {
        match Self::entry()?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Resolves an environment, stored, or newly prompted key in that order.
///
/// Newly prompted credentials are saved automatically. A secure-store failure
/// is returned as a warning so an interactive run can still proceed safely for
/// the current process.
pub fn resolve_persisted_api_key(
    environment_value: Option<String>,
    interaction: InteractionPolicy,
    prompt: &mut dyn CredentialPrompt,
    store: &dyn CredentialStore,
) -> Result<ResolvedApiKey, SettingsError> {
    if environment_value
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(ResolvedApiKey {
            api_key: resolve_api_key(environment_value, interaction, prompt)?,
            source: CredentialSource::Environment,
            store_warning: None,
        });
    }

    let mut warning = None;
    match store.load() {
        Ok(Some(secret)) => match ApiKey::new(secret) {
            Ok(api_key) => {
                return Ok(ResolvedApiKey {
                    api_key,
                    source: CredentialSource::SecureStore,
                    store_warning: None,
                });
            }
            Err(_) => {
                warning = Some("stored Mistral credential was invalid; replace it with `le-harness auth login`".into());
            }
        },
        Ok(None) => {}
        Err(error) => warning = Some(format!("could not read the OS credential store: {error}")),
    }

    let api_key = resolve_api_key(None, interaction, prompt)?;
    if let Err(error) = store.save(api_key.expose()) {
        warning = Some(format!(
            "could not save the Mistral API key in the OS credential store: {error}"
        ));
    }
    Ok(ResolvedApiKey {
        api_key,
        source: CredentialSource::Prompt,
        store_warning: warning,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct StubPrompt {
        value: String,
        calls: usize,
    }

    impl CredentialPrompt for StubPrompt {
        fn prompt_api_key(&mut self) -> Result<String, SettingsError> {
            self.calls += 1;
            Ok(self.value.clone())
        }
    }

    #[derive(Default)]
    struct MemoryStore {
        value: RefCell<Option<String>>,
        load_error: Option<String>,
        save_error: Option<String>,
    }

    impl CredentialStore for MemoryStore {
        fn load(&self) -> Result<Option<String>, String> {
            match &self.load_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.value.borrow().clone()),
            }
        }

        fn save(&self, secret: &str) -> Result<(), String> {
            if let Some(error) = &self.save_error {
                return Err(error.clone());
            }
            self.value.replace(Some(secret.into()));
            Ok(())
        }

        fn delete(&self) -> Result<bool, String> {
            Ok(self.value.replace(None).is_some())
        }
    }

    #[test]
    fn environment_precedes_store_and_prompt() {
        let store = MemoryStore {
            value: RefCell::new(Some("stored".into())),
            ..MemoryStore::default()
        };
        let mut prompt = StubPrompt {
            value: "prompted".into(),
            calls: 0,
        };

        let resolved = resolve_persisted_api_key(
            Some("environment".into()),
            InteractionPolicy::Allow,
            &mut prompt,
            &store,
        )
        .unwrap();

        assert_eq!(resolved.api_key.expose(), "environment");
        assert_eq!(resolved.source, CredentialSource::Environment);
        assert_eq!(prompt.calls, 0);
    }

    #[test]
    fn stored_key_is_reused_without_prompting() {
        let store = MemoryStore {
            value: RefCell::new(Some("stored".into())),
            ..MemoryStore::default()
        };
        let mut prompt = StubPrompt {
            value: "prompted".into(),
            calls: 0,
        };

        let resolved =
            resolve_persisted_api_key(None, InteractionPolicy::Allow, &mut prompt, &store).unwrap();

        assert_eq!(resolved.api_key.expose(), "stored");
        assert_eq!(resolved.source, CredentialSource::SecureStore);
        assert_eq!(prompt.calls, 0);
    }

    #[test]
    fn prompted_key_is_saved_for_the_next_run() {
        let store = MemoryStore::default();
        let mut prompt = StubPrompt {
            value: "prompted".into(),
            calls: 0,
        };

        let resolved =
            resolve_persisted_api_key(None, InteractionPolicy::Allow, &mut prompt, &store).unwrap();

        assert_eq!(resolved.source, CredentialSource::Prompt);
        assert_eq!(store.value.borrow().as_deref(), Some("prompted"));
        assert!(resolved.store_warning.is_none());
    }

    #[test]
    fn store_failure_warns_but_does_not_discard_prompted_key() {
        let store = MemoryStore {
            load_error: Some("locked".into()),
            save_error: Some("locked".into()),
            ..MemoryStore::default()
        };
        let mut prompt = StubPrompt {
            value: "prompted".into(),
            calls: 0,
        };

        let resolved =
            resolve_persisted_api_key(None, InteractionPolicy::Allow, &mut prompt, &store).unwrap();

        assert_eq!(resolved.api_key.expose(), "prompted");
        assert!(resolved.store_warning.unwrap().contains("could not save"));
    }
}
