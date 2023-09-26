use anyhow::{anyhow, Context, Result};
use pwd::Passwd;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs::File;

const CONFIG_ENV: &str = "DOORMAN_CONFIG";
const DEFAULT_CONFIG: &str = "/etc/doorman.yml";

#[derive(Serialize, Debug)]
pub struct User {
    pub uid: u32,
    pub username: String,
    pub display_name: String,
    pub is_sysop: bool,
}
impl User {}

#[derive(Deserialize, Debug)]
pub struct Options {
    pub user: String,
    pub dosemu_container: String,
    pub rundir: String,
    pub sysops: Vec<String>,
}

fn default_door_nodes() -> i8 {
    1
}

#[derive(Deserialize, Debug)]
pub struct Door {
    pub path: String,
    #[serde(default = "default_door_nodes")]
    pub nodes: i8,
    pub launch: String,
    pub configure: Option<String>,
    pub nightly: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub doorman: Options,
    pub doors: BTreeMap<String, Door>,
}
impl Config {
    fn config_path() -> Result<String> {
        if let Some(env_path) = env::var_os(CONFIG_ENV) {
            return match env_path.into_string() {
                Ok(value) => Ok(value),
                Err(_) => Err(anyhow!(
                    "Couldn't decode {}, is there garbage in your environment?",
                    CONFIG_ENV
                )),
            };
        }

        Ok(DEFAULT_CONFIG.to_string())
    }

    pub fn load() -> Result<Config> {
        let config_path = Config::config_path()?;

        let config_file = File::open(&config_path)
            .with_context(|| format!("Couldn't open config file {}", config_path))?;

        let config: Config = serde_yaml::from_reader(config_file)
            .with_context(|| format!("Couldn't open parse file {}", config_path))?;

        Ok(config)
    }

    fn make_display_name(pwent: &Passwd) -> String {
        if let Some(gecos) = pwent.gecos.clone() {
            if !gecos.is_empty() {
                return gecos.clone();
            }
        }

        pwent.name.clone()
    }

    fn pwent_to_user(&self, pwent: &Passwd) -> User {
        let display_name = Config::make_display_name(pwent);

        User {
            uid: pwent.uid,
            username: pwent.name.clone(),
            display_name,
            is_sysop: self.doorman.sysops.contains(&pwent.name),
        }
    }

    pub fn get_current_user(&self) -> Result<User> {
        let pwent = Passwd::current_user().with_context(|| "Couldn't lookup current user")?;
        Ok(self.pwent_to_user(&pwent))
    }

    pub fn get_user(&self, username: &String) -> Result<User> {
        if let Some(pwent) = Passwd::from_name(username)
            .with_context(|| format!("Couldn't lookup user {}", username))?
        {
            return Ok(self.pwent_to_user(&pwent));
        }
        Err(anyhow!("No such user: {}", username))
    }

    pub fn get_door(&self, door_name: &String) -> Result<&Door> {
        if let Some(door) = self.doors.get(door_name) {
            return Ok(door);
        }

        Err(anyhow!("Unknown door: {}", door_name))
    }
}
