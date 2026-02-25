use super::EnvironmentWrapper;

pub struct GuixShellEnvironment {
    pwd: Option<String>,
    packages: Vec<String>,
}

impl GuixShellEnvironment {
    pub fn new(pwd: Option<&str>, packages: Vec<String>) -> Self {
        Self {
            pwd: pwd.map(|s| s.to_string()),
            packages,
        }
    }
}

impl EnvironmentWrapper for GuixShellEnvironment {
    fn wrap_command(&self, cmd: &str, args: &[&str]) -> (String, Vec<String>) {
        let mut wrapped_args = vec!["shell".to_string()];

        if self.packages.is_empty() {
            wrapped_args.push("-m".to_string());
            wrapped_args.push("manifest.scm".to_string());
        } else {
            for pkg in &self.packages {
                wrapped_args.push(pkg.clone());
            }
        }

        wrapped_args.push("--".to_string());
        wrapped_args.push(cmd.to_string());
        wrapped_args.extend(args.iter().map(|s| s.to_string()));
        ("guix".to_string(), wrapped_args)
    }

    fn working_dir(&self) -> Option<&str> {
        self.pwd.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guix_wrap_manifest() {
        let env = GuixShellEnvironment::new(None, vec![]);
        let (cmd, args) = env.wrap_command("claude", &["--print"]);
        assert_eq!(cmd, "guix");
        assert_eq!(
            args,
            vec!["shell", "-m", "manifest.scm", "--", "claude", "--print"]
        );
    }

    #[test]
    fn test_guix_wrap_with_packages() {
        let env = GuixShellEnvironment::new(None, vec!["gh".to_string(), "curl".to_string()]);
        let (cmd, args) = env.wrap_command("gh", &["pr", "list"]);
        assert_eq!(cmd, "guix");
        assert_eq!(args, vec!["shell", "gh", "curl", "--", "gh", "pr", "list"]);
    }

    #[test]
    fn test_guix_wrap_with_args() {
        let env = GuixShellEnvironment::new(None, vec![]);
        let (cmd, args) = env.wrap_command("cargo", &["build", "--release"]);
        assert_eq!(cmd, "guix");
        assert_eq!(
            args,
            vec![
                "shell",
                "-m",
                "manifest.scm",
                "--",
                "cargo",
                "build",
                "--release"
            ]
        );
    }

    #[test]
    fn test_guix_working_dir() {
        let env = GuixShellEnvironment::new(Some("/home/franz/projects"), vec![]);
        assert_eq!(env.working_dir(), Some("/home/franz/projects"));
    }
}
