pub mod guix;
pub mod guix_container;
pub mod local;
pub mod podman;

use crate::config::types::EnvironmentSection;
use crate::error::Result;

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
    use crate::config::types::EnvironmentName;

    match config {
        None => Ok(Box::new(local::LocalEnvironment::new(None))),
        Some(section) => {
            let packages = section.packages.clone().unwrap_or_default();
            match section.name {
                EnvironmentName::Local => Ok(Box::new(local::LocalEnvironment::new(
                    section.pwd.as_deref(),
                ))),
                EnvironmentName::GuixShell => Ok(Box::new(guix::GuixShellEnvironment::new(
                    section.pwd.as_deref(),
                    packages,
                ))),
                EnvironmentName::GuixShellContainer => Ok(Box::new(
                    guix_container::GuixContainerEnvironment::new(section.pwd.as_deref(), packages),
                )),
                EnvironmentName::Podman => Ok(Box::new(podman::PodmanEnvironment::new(
                    section.pwd.as_deref(),
                    section.image.as_deref(),
                ))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::EnvironmentSection;

    use crate::config::types::EnvironmentName;

    fn env_config(name: EnvironmentName) -> EnvironmentSection {
        EnvironmentSection {
            name,
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
        let result = create_environment(Some(&env_config(EnvironmentName::Local)));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_guix_shell() {
        let result = create_environment(Some(&env_config(EnvironmentName::GuixShell)));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_guix_container() {
        let result = create_environment(Some(&env_config(EnvironmentName::GuixShellContainer)));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_env_podman() {
        let result = create_environment(Some(&env_config(EnvironmentName::Podman)));
        assert!(result.is_ok());
    }
}
