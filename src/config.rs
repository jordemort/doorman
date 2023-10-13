use super::container::ContainerEngine;
use super::user;
use anyhow::anyhow;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use log::{info, debug};
use nix::unistd;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize, Debug)]
struct DoormanOptions {
    /// The location of doorman's persistent data
    datadir: Option<PathBuf>,

    /// The location of doorman's lockfiles and other ephemeral data
    rundir: Option<PathBuf>,

    /// List of users that should be considered sysops
    sysops: Option<Vec<String>>,
}

fn default_dosemu_image() -> String {
    String::from("ghcr.io/jordemort/doorman-dosemu:main")
}

#[derive(Deserialize, Debug)]
struct ContainerOptions {
    /// Path to container engine binary, i.e. /path/to/podman or /path/to/docker
    engine_path: Option<PathBuf>,

    /// Set to true if you're using rootless podman
    rootless_podman: Option<bool>,

    #[serde(default = "default_dosemu_image")]
    /// Container image with dosemu; defaults to ghcr.io/jordemort/doorman-dosemu:main
    dosemu_image: String,
}

fn default_max_nodes() -> i8 {
    1
}

#[derive(Deserialize, Debug, Clone)]
pub struct DoorOptions {
    /// Path to door files; this will be mounted as drive Z: in DOSEMU
    pub door_path: PathBuf,

    #[serde(default = "default_max_nodes")]
    /// Number of concurrent players to allow.
    /// Make sure you have this many nodes configured in your door!
    /// Defaults to 1.
    pub max_nodes: i8,

    /// DOS command to lauch the door.
    pub launch_commands: String,

    /// DOS commands to launch the door's configuration program.
    pub configure_commands: Option<String>,

    /// DOS commands to run the door's nightly maintenence.
    pub nightly_commands: Option<String>,
}

pub struct Door {
    pub name: String,
    pub options: DoorOptions,
}

#[derive(Deserialize, Debug)]
struct ConfigFile {
    /// Options relating to doorman itself
    doorman: Option<DoormanOptions>,

    /// Options relating to how doorman runs containers
    container: Option<ContainerOptions>,

    /// Door definitions
    doors: HashMap<String, DoorOptions>,
}
impl ConfigFile {
    fn from_path(config_path: &PathBuf) -> Result<ConfigFile> {
        let config_file = fs::File::open(&config_path)
            .with_context(|| format!("Couldn't open config file: {}", config_path.display()))?;

        let config: ConfigFile = serde_yaml::from_reader(config_file)
            .with_context(|| format!("Couldn't parse config file: {}", config_path.display()))?;

        Ok(config)
    }
}

pub struct Config {
    pub datadir: PathBuf,
    pub rundir: PathBuf,
    pub user: user::User,
    pub dosemu_image: String,

    uid: unistd::Uid,
    gid: unistd::Gid,
    sysops: Vec<String>,
    doors: HashMap<String, DoorOptions>,
    engine: ContainerEngine,
}
impl Config {
    pub fn load() -> Result<Config> {
        let user = user::User::calling_user()?;

        info!("Running as user '{}' with UID {}", user.username, user.uid);

        let project_dirs = ProjectDirs::from("dev", "jordemort", "doorman").unwrap();
        let config_path = project_dirs.config_dir().join("doorman.yml");
        let config = ConfigFile::from_path(&config_path)?;

        let doorman = config.doorman.unwrap_or_else(|| DoormanOptions {
            datadir: None,
            rundir: None,
            sysops: None,
        });

        let datadir = doorman
            .datadir
            .unwrap_or(PathBuf::from(project_dirs.data_dir()));

        if !datadir.exists() {
            fs::create_dir_all(&datadir)
                .with_context(|| format!("Couldn't create datadir: {}", datadir.display()))?;
        }

        let rundir = doorman.rundir.unwrap_or(
            project_dirs
                .runtime_dir()
                .map_or(datadir.join("run"), |rundir| PathBuf::from(rundir)),
        );

        if !rundir.exists() {
            fs::create_dir_all(&rundir)
                .with_context(|| format!("Couldn't create rundir: {}", rundir.display()))?;
        }

        let container = config.container.unwrap_or_else(|| ContainerOptions {
            engine_path: None,
            rootless_podman: None,
            dosemu_image: default_dosemu_image(),
        });

        let engine = ContainerEngine::new(&container.engine_path, &container.rootless_podman)?;

        Ok(Config {
            datadir,
            rundir,
            user,
            dosemu_image: container.dosemu_image,
            uid: unistd::getuid(),
            gid: unistd::getgid(),
            sysops: doorman.sysops.unwrap_or(vec![]),
            doors: config.doors,
            engine,
        })
    }

    pub fn get_door(&self, name: &str) -> Result<Door> {
        let options = self
            .doors
            .get(name)
            .ok_or(anyhow!("Unknown door '{}'", name))?;

        Ok(Door {
            name: name.to_string(),
            options: options.clone(),
        })
    }

    pub fn is_sysop(&self) -> bool {
        if self.user.uid == self.uid.as_raw() || self.user.uid == 0 {
            true
        } else {
            self.sysops.contains(&self.user.username)
        }
    }

    pub fn switch_user(
        &mut self,
        username: &Option<String>,
        uid: Option<u32>,
        display_name: &Option<String>,
    ) -> Result<()> {
        if !self.is_sysop() {
            return Err(anyhow!("Only sysops can switch identities!"));
        }

        let mut user = self.user.clone();

        if let (Some(uid), Some(username)) = (uid, username) {
            user = user::User {
                uid,
                username: username.clone(),
                display_name: display_name.clone().unwrap_or_else(|| username.clone()),
            };
        } else {
            if let Some(uid) = uid {
                user = user::User::from_uid(unistd::Uid::from_raw(uid))?;
                if let Some(username) = username {
                    user.username = username.clone();
                }
            } else if let Some(username) = username {
                user = user::User::from_username(&username)?;
            }

            if let Some(display_name) = display_name {
                user.display_name = display_name.clone();
            }
        }

        self.user = user;

        Ok(())
    }

    fn run_args(
        &self,
        env: &HashMap<&str, String>,
        volumes: &HashMap<PathBuf, PathBuf>,
        labels: &HashMap<&str, String>,
    ) -> Vec<String> {
        let mut args: Vec<String> = vec![
            format!("--user={}:{}", self.uid, self.gid),
            "--tmpfs=/run/user".to_string(),
            "--tmpfs=/tmp".to_string(),
            "--tmpfs=/var/tmp".to_string(),
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

        if self.engine.rootless_podman {
            args.push("--userns=keep-id".to_string());
            args.push("--passwd=false".to_string());
        }

        debug!("Container run args: {:?}", args);

        args
    }

    pub fn container_command(&self, command: &str) -> Command {
        let mut cmd = Command::new(&self.engine.path);

        cmd.arg(command);
        cmd
    }

    pub fn run_container(
        &self,
        env: &HashMap<&str, String>,
        volumes: &HashMap<PathBuf, PathBuf>,
        labels: &HashMap<&str, String>,
    ) -> Command {
        let mut cmd = self.container_command("run");

        cmd.args(self.run_args(env, volumes, labels));
        cmd
    }
}
