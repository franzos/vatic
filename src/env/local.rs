use super::EnvironmentWrapper;

pub struct LocalEnvironment {
    pwd: Option<String>,
}

impl LocalEnvironment {
    pub fn new(pwd: Option<&str>) -> Self {
        Self {
            pwd: pwd.map(|s| s.to_string()),
        }
    }
}

impl EnvironmentWrapper for LocalEnvironment {
    fn wrap_command(&self, cmd: &str, args: &[&str]) -> (String, Vec<String>) {
        (
            cmd.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        )
    }

    fn working_dir(&self) -> Option<&str> {
        self.pwd.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_wrap_command() {
        let env = LocalEnvironment::new(None);
        let (cmd, args) = env.wrap_command("claude", &["--print", "--system-prompt", "Be nice."]);
        assert_eq!(cmd, "claude");
        assert_eq!(args, vec!["--print", "--system-prompt", "Be nice."]);
    }

    #[test]
    fn test_local_working_dir_none() {
        let env = LocalEnvironment::new(None);
        assert_eq!(env.working_dir(), None);
    }

    #[test]
    fn test_local_working_dir_some() {
        let env = LocalEnvironment::new(Some("/home/franz/projects"));
        assert_eq!(env.working_dir(), Some("/home/franz/projects"));
    }
}
