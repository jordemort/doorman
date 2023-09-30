use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json;
use std::path::PathBuf;
use std::process::Command;
use which::which;

#[derive(Deserialize, Debug)]
struct PodmanInfo {
    host: PodmanHost,
}

#[derive(Deserialize, Debug)]
struct PodmanHost {
    security: PodmanSecurity,
}

#[derive(Deserialize, Debug)]
struct PodmanSecurity {
    rootless: bool,
}

fn is_podman(path: &PathBuf) -> Result<bool> {
    let cmd = Command::new(path).arg("--version").output()?;
    let output = String::from_utf8(cmd.stdout)?.to_uppercase();

    Ok(output.starts_with("PODMAN "))
}

fn is_rootless_podman(path: &PathBuf) -> Result<bool> {
    if !is_podman(path)? {
        return Ok(false);
    }

    let cmd = Command::new(path)
        .arg("info")
        .arg("--format=json")
        .output()?;

    let output = String::from_utf8(cmd.stdout)?;

    if let Ok(info) = serde_json::from_str::<PodmanInfo>(&output) {
        Ok(info.host.security.rootless)
    } else {
        Ok(false)
    }
}

pub struct ContainerEngine {
    pub path: PathBuf,
    pub rootless_podman: bool,
}
impl ContainerEngine {
    pub fn new(
        engine_path: &Option<PathBuf>,
        rootless_podman: &Option<bool>,
    ) -> Result<ContainerEngine> {
        let path = engine_path.clone().unwrap_or_else(|| {
            which("podman").unwrap_or_else(|_| {
                which("docker")
                    .with_context(|| "Couldn't find podman or docker in PATH")
                    .unwrap()
            })
        });

        let rootless_podman = rootless_podman.unwrap_or_else(|| {
            is_rootless_podman(&path)
                .with_context(|| "Failed while checking for rootless podman")
                .unwrap()
        });

        Ok(ContainerEngine {
            path,
            rootless_podman,
        })
    }
}
