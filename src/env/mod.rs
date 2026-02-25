pub mod guix;
pub mod guix_container;
pub mod local;
pub mod podman;

use crate::config::types::EnvironmentSection;
use crate::error::{Error, Result};

pub trait EnvironmentWrapper: Send + Sync {
    /// One-time setup before first use (e.g. building a container image).
    fn ensure_ready(&self) -> Result<()> {
        Ok(())
    }

    /// Wrap a command so it runs inside this environment.
    fn wrap_command(&self, cmd: &str, args: &[&str]) -> (String, Vec<String>);

    /// Working directory override, if any.
    fn working_dir(&self) -> Option<&str>;
}

/// Build the right environment wrapper from config. Defaults to local.
pub fn create_environment(
    config: Option<&EnvironmentSection>,
) -> Result<Box<dyn EnvironmentWrapper>> {
    match config {
        None => Ok(Box::new(local::LocalEnvironment::new(None))),
        Some(section) => {
            let packages = section.packages.clone().unwrap_or_default();
            match section.name.as_str() {
                "local" => Ok(Box::new(local::LocalEnvironment::new(
                    section.pwd.as_deref(),
                ))),
                "guix-shell" => Ok(Box::new(guix::GuixShellEnvironment::new(
                    section.pwd.as_deref(),
                    packages,
                ))),
                "guix-shell-container" => Ok(Box::new(
                    guix_container::GuixContainerEnvironment::new(section.pwd.as_deref(), packages),
                )),
                "podman" => Ok(Box::new(podman::PodmanEnvironment::new(
                    section.pwd.as_deref(),
                    section.image.as_deref(),
                ))),
                other => Err(Error::Config(format!("unknown environment: '{other}'"))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::EnvironmentSection;

    fn env_config(name: &str) -> EnvironmentSection {
        EnvironmentSection {
            name: name.to_string(),
            pwd: None,
            packages: None,
            image: None,
        }
    }

    #[test]
    fn test_create_env_default_local() {
        let result = create_environment(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_local() {
        let result = create_environment(Some(&env_config("local")));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_guix_shell() {
        let result = create_environment(Some(&env_config("guix-shell")));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_guix_container() {
        let result = create_environment(Some(&env_config("guix-shell-container")));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_podman() {
        let result = create_environment(Some(&env_config("podman")));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_unknown() {
        let result = create_environment(Some(&env_config("bogus")));
        match result {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("unknown environment"),
                    "expected 'unknown environment' in: {msg}"
                );
            }
            Ok(_) => panic!("expected Err for unknown environment"),
        }
    }
}
