//use super::cfg::{Config, Door, User};
use super::config;
use super::dos::Templates;
use super::user::User;
use super::{LaunchArgs, SysopCmdArgs};
use log::debug;
use anyhow::{anyhow, Context, Result};
use chrono::Local;
use fs4::FileExt;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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
        if let Ok(envstr) = envar.into_string() {
            return envstr;
        }
    }

    String::from("xterm")
}

fn make_lockfile(path: &Path) -> Result<fs::File> {
    fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("Couldn't open lockfile {}", path.display()))
}

fn make_node_lockfile(
    max_nodes: i8,
    door_name: &str,
    config: &config::Config,
) -> Result<(i8, PathBuf, std::fs::File)> {
    let mut node: i8 = 1;

    while node <= max_nodes {
        let node_lockfile_path = config.rundir.join(format!("{0}.{1}.lock", door_name, node));
        let node_lockfile = make_lockfile(&node_lockfile_path)
            .with_context(|| format!("Failed to lock node {} for door '{}'", node, door_name))?;

        if node_lockfile.try_lock_exclusive().is_ok() {
            return Ok((node, node_lockfile_path, node_lockfile));
        }

        node += 1;
    }

    return Err(anyhow!("All nodes for {0} are busy!", door_name));
}

pub fn launch(args: &LaunchArgs, mut config: config::Config) -> Result<()> {
    let door = config.get_door(&args.door)?;

    if args.user.is_some() || args.user_id.is_some() || args.display_name.is_some() {
        config.switch_user(&args.user, args.user_id, &args.display_name)?;
    }

    let door_lockfile_path = config.rundir.join(format!("{}.lock", door.name));
    let door_lockfile = make_lockfile(&door_lockfile_path).with_context(|| "While locking door")?;

    if door_lockfile.try_lock_shared().is_err() {
        return Err(anyhow!(
            "Sorry, {0} is currently undergoing maintenence.",
            door.name
        ));
    }

    let (node, node_lockfile_path, node_lockfile) =
        make_node_lockfile(door.options.max_nodes, &door.name, &config)?;

    let node_rundir = config.rundir.join(format!("{0}.{1}", door.name, node));

    if node_rundir.exists() {
        fs::remove_dir_all(&node_rundir)
            .with_context(|| format!("Couldn't clean up node rundir {}", node_rundir.display()))?;
    }

    fs::create_dir_all(&node_rundir)
        .with_context(|| format!("Couldn't create node rundir {}", node_rundir.display()))?;

    let vars = LaunchVars {
        user: &config.user,
        node,
        current_time: Local::now().format("%H:%M").to_string(),
    };

    let templates = Templates::new();

    templates.write_dos("door.sys", &node_rundir, &vars)?;

    let commands = BatchCommands {
        commands: templates
            .render_string(&door.options.launch_commands, &vars)
            .with_context(|| format!("Couldn't generate batch commands for {}", door.name))?,
    };

    templates.write_dos("doorman.bat", &node_rundir, &commands)?;

    let env = HashMap::from([
        ("TERM", get_term()),
        (
            "DOORMAN_RAW",
            if args.raw {
                "1".to_string()
            } else {
                "0".to_string()
            },
        ),
    ]);

    let volumes = HashMap::from([
        (node_rundir.clone(), PathBuf::from("/mnt/doorman")),
        (door.options.door_path.clone(), PathBuf::from("/mnt/door")),
        (door_lockfile_path.clone(), PathBuf::from("/mnt/door.lock")),
        (node_lockfile_path, PathBuf::from("/mnt/node.lock")),
    ]);

    let labels = HashMap::from([
        ("doorman.door", door.name.clone()),
        ("doorman.node", format!("{}", node)),
        ("doorman.user", config.user.username.clone()),
        (
            "doorman.rundir",
            format!("{}", node_rundir.clone().display()),
        ),
    ]);

    let run = config
        .run_container(&env, &volumes, &labels)
        .arg("-d")
        .arg(&config.dosemu_image)
        .arg("wait-for-launch.sh")
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("While starting container for door '{}'", door.name))?;

    let run_output = run
        .wait_with_output()
        .with_context(|| format!("Failed to start container for door '{}'", door.name))?;

    if run_output.status.code() != Some(0) {
        if let Some(code) = run_output.status.code() {
            return Err(anyhow!(
                "Starting container for {} failed with exit code {}",
                door.name,
                code
            ));
        } else {
            return Err(anyhow!(
                "Starting container for {} failed with an unknown exit code",
                door.name
            ));
        }
    }

    let container_id =
        String::from_utf8(run_output.stdout).with_context(|| "While decoding container ID")?;

    debug!("Container ID: {0}", container_id.trim());

    node_lockfile.unlock()?;

    config
        .container_command("exec")
        .arg("-ti")
        .arg(container_id.trim())
        .arg("launch.sh")
        .status()
        .with_context(|| "While starting client")?;

    Ok(())
}

pub fn configure(args: &SysopCmdArgs, config: &config::Config) -> Result<()> {
    let door = config.get_door(&args.door)?;
    sysop_command(
        args,
        config,
        &door,
        "configure",
        &door.options.configure_commands,
    )
}

pub fn nightly(args: &SysopCmdArgs, config: &config::Config) -> Result<()> {
    let door = config.get_door(&args.door)?;
    sysop_command(
        args,
        config,
        &door,
        "nightly",
        &door.options.nightly_commands,
    )
}

fn sysop_command(
    args: &SysopCmdArgs,
    config: &config::Config,
    door: &config::Door,
    command: &str,
    template: &Option<String>,
) -> Result<()> {
    if !config.is_sysop() {
        return Err(anyhow!("This command is only for sysops!"));
    }

    if template.is_none() {
        return Err(anyhow!(
            "No {} command configured for {}!",
            command,
            door.name
        ));
    }

    let door_lockfile_path = config.rundir.join(format!("{}.lock", door.name));
    let door_lockfile = make_lockfile(&door_lockfile_path)?;

    if args.nowait {
        if door_lockfile.try_lock_exclusive().is_err() {
            return Err(anyhow!(
                "Sorry, I couldn't lock the door '{}' exclusively.",
                door.name
            ));
        }
    } else {
        door_lockfile.lock_exclusive()?;
    }

    let sysop_rundir = config.rundir.join(format!("{}.sysop", door.name));

    if sysop_rundir.exists() {
        fs::remove_dir_all(&sysop_rundir).with_context(|| {
            format!("Couldn't clean up sysop rundir {}", sysop_rundir.display())
        })?;
    }

    fs::create_dir_all(&sysop_rundir)
        .with_context(|| format!("Couldn't create sysop rundir {}", sysop_rundir.display()))?;

    let templates = Templates::new();
    let commands = BatchCommands {
        commands: template.clone().unwrap(),
    };

    templates.write_dos("doorman.bat", &sysop_rundir, commands)?;

    let env = HashMap::from([("TERM", get_term())]);

    let volumes = HashMap::from([
        (sysop_rundir.clone(), PathBuf::from("/mnt/doorman")),
        (door.options.door_path.clone(), PathBuf::from("/mnt/door")),
        (door_lockfile_path, PathBuf::from("/mnt/door.lock")),
    ]);

    let labels = HashMap::from([
        ("doorman.door", door.name.clone()),
        ("doorman.command", command.to_string()),
        ("doorman.user", config.user.username.clone()),
        (
            "doorman.rundir",
            format!("{}", sysop_rundir.clone().display()),
        ),
    ]);

    let mut run = config
        .run_container(&env, &volumes, &labels)
        .arg("-ti")
        .arg(&config.dosemu_image)
        .arg(format!("{}.sh", command))
        .spawn()
        .with_context(|| format!("While spawning container for door '{}'", door.name))?;

    door_lockfile.unlock()?;

    run.wait()
        .with_context(|| format!("While waiting for container for door '{}'", door.name))?;

    Ok(())
}
