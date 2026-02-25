use crate::error::{Error, Result};

use super::EnvironmentWrapper;

const DEFAULT_IMAGE: &str = "vatic-agent:latest";
const DEFAULT_PWD: &str = "/tmp";

const DOCKERFILE: &str = "\
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    curl \
    git \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://claude.ai/install.sh | bash
";

pub struct PodmanEnvironment {
    pwd: Option<String>,
    image: String,
}

impl PodmanEnvironment {
    pub fn new(pwd: Option<&str>, image: Option<&str>) -> Self {
        Self {
            pwd: pwd.map(|s| s.to_string()),
            image: image.unwrap_or(DEFAULT_IMAGE).to_string(),
        }
    }

    /// Falls back to /tmp if no pwd is configured.
    fn effective_pwd(&self) -> &str {
        self.pwd.as_deref().unwrap_or(DEFAULT_PWD)
    }

    fn home_dir() -> Option<String> {
        std::env::var("HOME").ok()
    }

    /// Mount a path into the container if it exists on the host.
    fn mount_if_exists(args: &mut Vec<String>, path: &str) {
        if std::path::Path::new(path).exists() {
            args.push("-v".to_string());
            args.push(format!("{path}:{path}"));
        }
    }

    /// Quick check — does this image already exist locally?
    fn image_exists(image: &str) -> bool {
        std::process::Command::new("podman")
            .args(["image", "exists", image])
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Build our default image from the embedded Dockerfile.
    fn build_image(image: &str) -> Result<()> {
        tracing::info!("building podman image '{image}' (first-time setup)");

        let build_dir = std::env::temp_dir().join("vatic-podman-build");
        std::fs::create_dir_all(&build_dir)
            .map_err(|e| Error::Environment(format!("cannot create build dir: {e}")))?;
        let dockerfile = build_dir.join("Dockerfile");
        std::fs::write(&dockerfile, DOCKERFILE)
            .map_err(|e| Error::Environment(format!("cannot write Dockerfile: {e}")))?;

        let output = std::process::Command::new("podman")
            .args(["build", "-t", image])
            .arg(&build_dir)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|e| Error::Environment(format!("cannot run podman build: {e}")))?;

        let _ = std::fs::remove_dir_all(&build_dir);

        if !output.status.success() {
            return Err(Error::Environment(format!(
                "podman build failed with {}",
                output.status
            )));
        }

        tracing::info!("podman image '{image}' built successfully");
        Ok(())
    }
}

impl EnvironmentWrapper for PodmanEnvironment {
    fn ensure_ready(&self) -> Result<()> {
        // Only auto-build our default image — custom images are the user's problem
        if self.image == DEFAULT_IMAGE && !Self::image_exists(&self.image) {
            Self::build_image(&self.image)?;
        }
        Ok(())
    }

    fn wrap_command(&self, cmd: &str, args: &[&str]) -> (String, Vec<String>) {
        let pwd = self.effective_pwd();
        let volume = format!("{pwd}:{pwd}");

        let mut wrapped_args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--network=host".to_string(),
            "-v".to_string(),
            volume,
            "-w".to_string(),
            pwd.to_string(),
        ];

        // Claude Code needs ~/.claude for auth
        if let Some(home) = Self::home_dir() {
            Self::mount_if_exists(&mut wrapped_args, &format!("{home}/.claude"));
        }

        wrapped_args.push(self.image.clone());
        wrapped_args.push(cmd.to_string());
        wrapped_args.extend(args.iter().map(|s| s.to_string()));
        ("podman".to_string(), wrapped_args)
    }

    fn working_dir(&self) -> Option<&str> {
        self.pwd.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_podman_wrap_command() {
        let env = PodmanEnvironment::new(Some("/home/franz/news"), None);
        let (cmd, args) = env.wrap_command("claude", &["--print"]);
        assert_eq!(cmd, "podman");
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"--network=host".to_string()));
        assert!(args.contains(&"-w".to_string()));
        assert!(args.contains(&"/home/franz/news".to_string()));
        assert!(args.contains(&"vatic-agent:latest".to_string()));
        let img_pos = args.iter().position(|a| a == "vatic-agent:latest").unwrap();
        assert_eq!(args[img_pos + 1], "claude");
        assert_eq!(args[img_pos + 2], "--print");
    }

    #[test]
    fn test_podman_custom_image() {
        let env = PodmanEnvironment::new(Some("/tmp"), Some("node:22-slim"));
        let (cmd, args) = env.wrap_command("node", &["index.js"]);
        assert_eq!(cmd, "podman");
        assert!(args.contains(&"node:22-slim".to_string()));
        assert!(!args.contains(&"vatic-agent:latest".to_string()));
    }

    #[test]
    fn test_podman_wrap_command_no_pwd() {
        let env = PodmanEnvironment::new(None, None);
        let (cmd, args) = env.wrap_command("cargo", &["build"]);
        assert_eq!(cmd, "podman");
        assert!(args.contains(&"/tmp:/tmp".to_string()));
    }

    #[test]
    fn test_podman_mounts_claude_dir() {
        if let Some(home) = PodmanEnvironment::home_dir() {
            let claude_dir = format!("{home}/.claude");
            if std::path::Path::new(&claude_dir).exists() {
                let env = PodmanEnvironment::new(None, None);
                let (_, args) = env.wrap_command("claude", &["--print"]);
                assert!(args.contains(&format!("{claude_dir}:{claude_dir}")));
            }
        }
    }

    #[test]
    fn test_podman_network_host() {
        let env = PodmanEnvironment::new(None, None);
        let (_, args) = env.wrap_command("curl", &["http://localhost"]);
        assert!(args.contains(&"--network=host".to_string()));
    }

    #[test]
    fn test_podman_working_dir() {
        let env = PodmanEnvironment::new(Some("/home/franz/projects"), None);
        assert_eq!(env.working_dir(), Some("/home/franz/projects"));
    }

    #[test]
    fn test_ensure_ready_custom_image_skips_build() {
        // Custom images shouldn't trigger auto-build
        let env = PodmanEnvironment::new(None, Some("my-custom:latest"));
        // Should succeed without attempting to build
        assert!(env.ensure_ready().is_ok());
    }
}
