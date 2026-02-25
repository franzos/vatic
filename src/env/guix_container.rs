use super::EnvironmentWrapper;

// Minimum set of packages for Claude Code to work inside a container
const DEFAULT_PACKAGES: &[&str] = &[
    "coreutils",
    "bash",
    "grep",
    "sed",
    "gawk",
    "git",
    "node",
    "claude-code",
    "nss-certs",
];

pub struct GuixContainerEnvironment {
    pwd: Option<String>,
    packages: Vec<String>,
}

impl GuixContainerEnvironment {
    pub fn new(pwd: Option<&str>, packages: Vec<String>) -> Self {
        Self {
            pwd: pwd.map(|s| s.to_string()),
            packages,
        }
    }

    fn home_dir() -> Option<String> {
        std::env::var("HOME").ok()
    }

    /// Share a path into the container if it exists on the host.
    fn share_if_exists(args: &mut Vec<String>, path: &str) {
        if std::path::Path::new(path).exists() {
            args.push(format!("--share={path}"));
        }
    }
}

impl EnvironmentWrapper for GuixContainerEnvironment {
    fn wrap_command(&self, cmd: &str, args: &[&str]) -> (String, Vec<String>) {
        let mut wa = vec!["shell".to_string(), "--container".to_string()];

        wa.push("--network".to_string());

        // Claude Code needs ~/.claude for auth
        if let Some(home) = Self::home_dir() {
            Self::share_if_exists(&mut wa, &format!("{home}/.claude"));
        }

        if let Some(pwd) = &self.pwd {
            wa.push(format!("--share={pwd}"));
        }

        wa.push("--preserve=^COLORTERM$".to_string());

        if self.packages.is_empty() {
            for pkg in DEFAULT_PACKAGES {
                wa.push(pkg.to_string());
            }
        } else {
            for pkg in &self.packages {
                wa.push(pkg.clone());
            }
        }

        wa.push("--".to_string());
        wa.push(cmd.to_string());
        wa.extend(args.iter().map(|s| s.to_string()));
        ("guix".to_string(), wa)
    }

    fn working_dir(&self) -> Option<&str> {
        self.pwd.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_defaults() {
        let env = GuixContainerEnvironment::new(None, vec![]);
        let (cmd, args) = env.wrap_command("claude", &["--print"]);
        assert_eq!(cmd, "guix");
        assert!(args.contains(&"--container".to_string()));
        assert!(args.contains(&"--network".to_string()));
        assert!(args.contains(&"--preserve=^COLORTERM$".to_string()));
        for pkg in DEFAULT_PACKAGES {
            assert!(
                args.contains(&pkg.to_string()),
                "missing default package: {pkg}"
            );
        }
        let sep = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[sep + 1], "claude");
        assert_eq!(args[sep + 2], "--print");
    }

    #[test]
    fn test_container_shares_claude_dir() {
        if let Some(home) = GuixContainerEnvironment::home_dir() {
            let claude_dir = format!("{home}/.claude");
            if std::path::Path::new(&claude_dir).exists() {
                let env = GuixContainerEnvironment::new(None, vec![]);
                let (_, args) = env.wrap_command("claude", &["--print"]);
                assert!(args.contains(&format!("--share={claude_dir}")));
            }
        }
    }

    #[test]
    fn test_container_custom_packages() {
        let env = GuixContainerEnvironment::new(
            None,
            vec!["rust".to_string(), "gcc-toolchain".to_string()],
        );
        let (cmd, args) = env.wrap_command("cargo", &["build"]);
        assert_eq!(cmd, "guix");
        assert!(args.contains(&"rust".to_string()));
        assert!(args.contains(&"gcc-toolchain".to_string()));
        assert!(!args.contains(&"claude-code".to_string()));
    }

    #[test]
    fn test_container_with_pwd() {
        let env =
            GuixContainerEnvironment::new(Some("/home/franz/project"), vec!["node".to_string()]);
        let (_, args) = env.wrap_command("node", &["index.js"]);
        assert!(args.contains(&"--share=/home/franz/project".to_string()));
    }

    #[test]
    fn test_container_working_dir() {
        let env = GuixContainerEnvironment::new(Some("/home/franz/projects"), vec![]);
        assert_eq!(env.working_dir(), Some("/home/franz/projects"));
    }
}
