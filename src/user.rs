use anyhow::{anyhow, Result};
use nix::unistd;
use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
pub struct User {
    pub uid: u32,
    pub username: String,
    pub display_name: String,
}
impl User {
    fn from_pwent(pwent: &unistd::User) -> Result<User> {
        let gecos = pwent.gecos.clone().into_string()?;
        let gecos_name = gecos.split(",").next().unwrap_or("");

        let display_name = if gecos.is_empty() {
            pwent.name.clone()
        } else {
            gecos_name.to_string()
        };

        Ok(User {
            uid: pwent.uid.as_raw(),
            username: pwent.name.clone(),
            display_name,
        })
    }

    pub fn from_uid(uid: unistd::Uid) -> Result<User> {
        let pwent =
            unistd::User::from_uid(uid)?.ok_or(anyhow!("Couldn't look up user ID {}", uid))?;
        User::from_pwent(&pwent)
    }

    pub fn from_username(username: &str) -> Result<User> {
        let pwent = unistd::User::from_name(username)?
            .ok_or(anyhow!("Couldn't look up username '{}'", username))?;
        User::from_pwent(&pwent)
    }

    pub fn from_current_uid() -> Result<User> {
        let uid = unistd::getuid();
        User::from_uid(uid)
    }
}
