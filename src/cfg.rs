use pwd::Passwd;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::File;
use std::collections::BTreeMap;

const CONFIG_ENV: &str = "DOORMAN_CONFIG";
const DEFAULT_CONFIG: &str = "/etc/doorman.yml";

#[derive(Deserialize, Debug)]
pub struct Options {
    pub user: String,
    pub dosemu_container: String,
    pub rundir: String,
    pub sysops: Vec<String>,
}

fn default_door_nodes() -> i8 {
    return 1;
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

#[derive(Serialize, Debug)]
pub struct User {
    pub uid: u32,
    pub username: String,
    pub display_name: String,
    pub is_sysop: bool,
}

impl Config {
    pub fn config_path() -> String {
        let envvar: Option<String> = match env::var_os(CONFIG_ENV) {
            Some(val) => match val.to_str() {
                Some(val_to_str) => Some(String::from(val_to_str)),
                None => None,
            },
            None => None,
        };

        return String::from(match envvar {
            Some(val) => val,
            None => String::from(DEFAULT_CONFIG),
        });
    }

    pub fn load() -> Result<Config, String> {
        let config_path = Config::config_path();
        let config_file = match File::open(&config_path) {
            Ok(file) => file,
            Err(e) => {
                return Err(format!(
                    "Couldn't load config from {0}: {1}",
                    config_path, e
                ))
            }
        };

        let config: Config = match serde_yaml::from_reader(config_file) {
            Ok(yaml) => yaml,
            Err(e) => {
                return Err(format!(
                    "Couldn't parse config from {0}: {1}",
                    config_path, e
                ))
            }
        };

        return Ok(config);
    }

    pub fn get_user(&self, username: Option<&String>) -> Result<User, String> {
        let pwent = match username {
            Some(username) => match Passwd::from_name(&username) {
                Ok(pwent) => match pwent {
                    Some(pwent) => pwent,
                    None => return Err(format!("No such user: {0}", username)),
                },
                Err(_) => return Err(format!("Couldn't lookup user {0}", username)),
            },
            None => match Passwd::current_user() {
                Some(pwent) => pwent,
                None => return Err(String::from("Couldn't lookup current user")),
            },
        };

        let display_name = match pwent.gecos {
            Some(gecos) => {
                if gecos.len() > 0 {
                    gecos
                } else {
                    pwent.name.clone()
                }
            }
            None => pwent.name.clone(),
        };

        return Ok(User {
            uid: pwent.uid,
            username: pwent.name.clone(),
            display_name: display_name,
            is_sysop: self.doorman.sysops.contains(&pwent.name),
        });
    }
}
