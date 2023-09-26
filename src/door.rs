use super::cfg::{Config, User};
use super::dos::Templates;
use super::{LaunchArgs, SysopCmdArgs};

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use fs4::FileExt;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Serialize, Debug)]
struct LaunchVars<'a> {
    user: &'a User,
    node: i8,
    current_time: String,
}

#[derive(Serialize, Debug)]
struct BatchCommands {
    commands: String,
}

fn get_term() -> String {
    if let Some(envar) = env::var_os("TERM") {
        let maybe_valid = envar.into_string();
        if maybe_valid.is_ok() {
            return maybe_valid.unwrap();
        }
    }

    return String::from("xterm");
}

pub fn launch(args: &LaunchArgs, config: &Config) -> Result<()> {
    let door = config.get_door(&args.door)?;
    let mut user = config.get_current_user()?;

    if let Some(switch_user) = args.user.clone() {
        if !user.is_sysop {
            return Err(anyhow!("You can't switch users if you're not a sysop!",));
        }
        user = config.get_user(&switch_user)?;
    }

    let rundir = Path::new(&config.doorman.rundir);

    fs::create_dir_all(rundir)
        .with_context(|| format!("Couldn't create rundir {}", rundir.display()))?;

    let door_lockfile_path = rundir.join(format!("{0}.lock", args.door));
    let door_lockfile = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&door_lockfile_path)
        .with_context(|| {
            format!(
                "Couldn't open door lockfile {}",
                door_lockfile_path.display()
            )
        })?;

    if door_lockfile.try_lock_shared().is_err() {
        return Err(anyhow!(
            "Sorry, {0} is currently undergoing maintenence.",
            args.door
        ));
    }

    let mut node: i8 = 1;
    let mut found_node = false;

    while (node <= door.nodes) && !found_node {
        let node_lockfile_path = rundir.join(format!("{0}.{1}.lock", args.door, node));
        let node_lockfile = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&node_lockfile_path)
            .with_context(|| {
                format!(
                    "Couldn't open node lockfile {}",
                    node_lockfile_path.display()
                )
            })?;

        if node_lockfile.try_lock_exclusive().is_err() {
            node += 1;
            continue;
        }

        found_node = true;

        let node_rundir = rundir.join(format!("{0}.{1}", args.door, node));

        if node_rundir.exists() {
            fs::remove_dir_all(&node_rundir).with_context(|| {
                format!("Couldn't clean up node rundir {}", node_rundir.display())
            })?;
        }

        fs::create_dir_all(&node_rundir)
            .with_context(|| format!("Couldn't create node rundir {}", node_rundir.display()))?;

        let vars = LaunchVars {
            user: &user,
            node,
            current_time: Local::now().format("%H:%M").to_string(),
        };

        let templates = Templates::new();

        templates.write_dos("door.sys", &node_rundir, &vars)?;

        let commands = BatchCommands {
            commands: templates
                .render_string(&door.launch, &vars)
                .with_context(|| format!("Couldn't generate batch commands for {}", args.door))?,
        };

        templates.write_dos("doorman.bat", &node_rundir, &commands)?;

        let run = Command::new("docker")
            .arg("run")
            .arg("-d")
            .arg("-p5901:5901")
            .arg(format!("-v{0}:/mnt/doorman", node_rundir.display()))
            .arg(format!("-v{0}:/mnt/door", door.path))
            .arg(format!("-eTERM={0}", get_term()))
            .arg(format!("-eDOORMAN_DOOR={0}", args.door))
            .arg(format!("-eDOORMAN_USER={0}", user.username))
            .arg(format!("-eDOORMAN_UID={0}", user.uid))
            .arg(format!(
                "-eDOORMAN_SYSOP={0}",
                if user.is_sysop { "1" } else { "0" }
            ))
            .arg(format!(
                "-eDOORMAN_RAW={0}",
                if args.raw { "1" } else { "0" }
            ))
            .arg(config.doorman.dosemu_container.as_str())
            .arg("wait-for-launch.sh")
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("While starting container for door '{}'", args.door))?;

        let run_output = run
            .wait_with_output()
            .with_context(|| format!("Failed to start container for door '{}'", args.door))?;

        if run_output.status.code() != Some(0) {
            if let Some(code) = run_output.status.code() {
                return Err(anyhow!(
                    "Starting container for {} failed with exit code {}",
                    args.door,
                    code
                ));
            } else {
                return Err(anyhow!(
                    "Starting container for {} failed with an unknown exit code",
                    args.door
                ));
            }
        }

        let container_id =
            String::from_utf8(run_output.stdout).with_context(|| "While decoding container ID")?;

        println!("Container ID = {0}", container_id.trim());

        Command::new("docker")
            .arg("exec")
            .arg("-ti")
            .arg(container_id.trim())
            .arg("launch.sh")
            .status()
            .with_context(|| "While starting client")?;
    }

    if !found_node {
        return Err(anyhow!("All nodes for {0} are busy!", args.door));
    }

    return Ok(());
}

pub fn configure(args: &SysopCmdArgs, config: &Config) -> Result<()> {
    println!("LOL not implemented yet");
    return Ok(());
}

pub fn nightly(args: &SysopCmdArgs, config: &Config) -> Result<()> {
    println!("LOL not implemented");
    return Ok(());
}
