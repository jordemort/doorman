use anyhow::{anyhow, Context, Result};
use nix::unistd;
use std::env;

pub fn setuid() -> Result<()> {
    let uid = unistd::getuid();
    let euid = unistd::geteuid();

    if euid != uid {
        unistd::setresuid(euid, euid, uid)
            .with_context(|| format!("Couldn't change user ID to {}", euid))?;

        let pwent = unistd::User::from_uid(uid)?
            .ok_or(anyhow!("Couldn't look up setuid user ID {}", uid))?;

        env::set_var("LOGNAME", &pwent.name);
        env::set_var("USER", &pwent.name);
        env::set_var("HOME", pwent.dir);
        env::remove_var("XDG_RUNTIME_DIR");
        env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    }

    let gid = unistd::getgid();
    let egid = unistd::getegid();

    if egid != gid {
        unistd::setresgid(egid, egid, gid)
            .with_context(|| format!("Couldn't change group ID to {}", egid))?;
    }

    Ok(())
}
