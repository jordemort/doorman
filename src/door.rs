use super::cfg::{Config, Door, User};
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

    let door_lockfile_path = rundir.join(format!("{}.lock", args.door));
    let door_lockfile = make_lockfile(&door_lockfile_path).with_context(|| "While locking door")?;

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
        let node_lockfile =
            make_lockfile(&node_lockfile_path).with_context(|| "While locking node")?;

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
            .arg(format!("-v{0}:/mnt/doorman", node_rundir.display()))
            .arg(format!("-v{0}:/mnt/door", door.path))
            .arg(format!(
                "-v{0}:/mnt/door.lock",
                door_lockfile_path.display()
            ))
            .arg(format!(
                "-v{0}:/mnt/node.lock",
                node_lockfile_path.display()
            ))
            .arg(format!("-eTERM={0}", get_term()))
            .arg(format!(
                "-eDOORMAN_RAW={0}",
                if args.raw { "1" } else { "0" }
            ))
            .arg(format!("--label=doorman.door={0}", args.door))
            .arg(format!("--label=doorman.node={0}", node))
            .arg(format!("--label=doorman.user={0}", user.username))
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

        node_lockfile.unlock()?;

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

    Ok(())
}

pub fn configure(args: &SysopCmdArgs, config: &Config) -> Result<()> {
    let door = config.get_door(&args.door)?;
    sysop_command(args, config, door, "configure", &door.configure)
}

pub fn nightly(args: &SysopCmdArgs, config: &Config) -> Result<()> {
    let door = config.get_door(&args.door)?;
    sysop_command(args, config, door, "nightly", &door.nightly)
}

fn sysop_command(
    args: &SysopCmdArgs,
    config: &Config,
    door: &Door,
    command: &str,
    template: &Option<String>,
) -> Result<()> {
    let user = config.get_current_user()?;

    if !user.is_sysop {
        return Err(anyhow!("This command is only for sysops!"));
    }

    if template.is_none() {
        return Err(anyhow!(
            "No {} command configured for {}!",
            command,
            args.door
        ));
    }

    let rundir = Path::new(&config.doorman.rundir);
    let door_lockfile_path = rundir.join(format!("{}.lock", args.door));
    let door_lockfile = make_lockfile(&door_lockfile_path)?;

    if args.nowait {
        if door_lockfile.try_lock_exclusive().is_err() {
            return Err(anyhow!(
                "Sorry, I couldn't lock the door '{}' exclusively.",
                args.door
            ));
        }
    } else {
        door_lockfile.lock_exclusive()?;
    }

    let sysop_rundir = rundir.join(format!("{}.sysop", args.door));

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

    let mut run = Command::new("docker")
        .arg("run")
        .arg("-ti")
        .arg(format!("-v{0}:/mnt/doorman", sysop_rundir.display()))
        .arg(format!("-v{0}:/mnt/door", door.path))
        .arg(format!(
            "-v{0}:/mnt/door.lock",
            door_lockfile_path.display()
        ))
        .arg(format!("-eTERM={0}", get_term()))
        .arg(format!("--label=doorman.door={0}", args.door))
        .arg(format!("--label=doorman.node={0}", command))
        .arg(format!("--label=doorman.user={0}", user.username))
        .arg(config.doorman.dosemu_container.as_str())
        .arg(format!("{}.sh", command))
        .spawn()
        .with_context(|| format!("While spawning container for door '{}'", args.door))?;

    door_lockfile.unlock()?;

    run.wait()
        .with_context(|| format!("While waiting for container for door '{}'", args.door))?;

    Ok(())
}
