use anyhow::{anyhow, Result};
use nix::unistd::{getegid, geteuid};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

enum ContainerEngineType {
    Podman,
    Docker,
}

pub struct ContainerEngine {
    pub path: PathBuf,
    uid: u32,
    gid: u32,
    engine_type: ContainerEngineType,
}

impl ContainerEngine {
    pub fn new(path: &PathBuf) -> Result<ContainerEngine> {
        let version = Command::new(path).arg("--version").output()?;
        let output = String::from_utf8(version.stdout)?.to_uppercase();
        let mut engine_type: Option<ContainerEngineType> = None;

        if output.starts_with("PODMAN ") {
            engine_type = Some(ContainerEngineType::Podman);
        } else if output.starts_with("DOCKER ") {
            engine_type = Some(ContainerEngineType::Docker);
        }

        if let Some(engine_type) = engine_type {
            Ok(ContainerEngine {
                path: path.clone(),
                uid: geteuid().as_raw(),
                gid: getegid().as_raw(),
                engine_type,
            })
        } else {
            Err(anyhow!(
                "Couldn't determine if {} is docker or podman",
                path.display()
            ))
        }
    }

    pub fn run(
        &self,
        env: &HashMap<&str, String>,
        volumes: &HashMap<PathBuf, PathBuf>,
        labels: &HashMap<&str, String>,
    ) -> Command {
        let mut args: Vec<String> = vec![
            "run".to_string(),
            format!("--user={}:{}", self.uid, self.gid),
        ];

        for (host_path, container_path) in volumes.iter() {
            args.push(format!(
                "-v{}:{}",
                host_path.display(),
                container_path.display()
            ));
        }

        for (key, value) in env.iter() {
            args.push(format!("-e{}={}", key, value));
        }

        for (key, value) in labels.iter() {
            args.push(format!("-l{}={}", key, value));
        }

        if matches!(self.engine_type, ContainerEngineType::Podman) {
            args.push("--userns=keep-id".to_string());
            args.push("--passwd=false".to_string());
        }

        let mut cmd = Command::new(&self.path);

        cmd.args(args);
        cmd
    }
}
