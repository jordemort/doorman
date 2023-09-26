use super::cfg::{Config, Door, User};
use super::dos::Templates;
use super::{LaunchArgs, SysopCmdArgs};

use chrono::Local;
use fs4::FileExt;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};

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

fn get_door<'a>(name: &String, config: &'a Config) -> &'a Door {
    let door = match config.doors.get(name) {
        Some(door) => door,
        None => {
            eprintln!("Unknown door: {0}", name);
            std::process::exit(1);
        }
    };

    return door;
}

fn get_caller(config: &Config) -> User {
    let user = match config.get_user(None) {
        Ok(user) => user,
        Err(e) => {
            eprintln!("{0}", e);
            std::process::exit(1);
        }
    };

    return user;
}

fn switch_user(user: User, config: &Config, to_user: &Option<String>) -> User {
    return match &to_user {
        Some(username) => {
            if !user.is_sysop {
                eprintln!("You can't switch users if you're not a sysop!");
                std::process::exit(1);
            }
            match config.get_user(Some(&username)) {
                Ok(user) => user,
                Err(e) => {
                    eprintln!("{0}", e);
                    std::process::exit(1);
                }
            }
        }
        None => user,
    };
}

fn get_container_id(child: Child) -> String {
    return match child.wait_with_output() {
        Ok(output) => match output.status.code() {
            Some(code) => {
                if code != 0 {
                    eprintln!("Starting container failed with status {0}", code);
                    std::process::exit(1);
                }
                match String::from_utf8(output.stdout) {
                    Ok(container_id) => container_id,
                    Err(e) => {
                        eprintln!("Couldn't decode container ID: {0}", e);
                        std::process::exit(1);
                    }
                }
            }
            None => {
                eprintln!("Starting container failed with unknown status");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Starting container failed: {0}", e);
            std::process::exit(1);
        }
    };
}

pub fn launch(args: &LaunchArgs, config: &Config) {
    let door = get_door(&args.door, config);
    let user = switch_user(get_caller(config), config, &args.user);

    let rundir = Path::new(&config.doorman.rundir);

    match fs::create_dir_all(rundir) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("Couldn't create rundir {0}: {1}", rundir.display(), e);
            std::process::exit(1);
        }
    };

    let door_lockfile_path = rundir.join(format!("{0}.lock", args.door));
    let door_lockfile = match fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&door_lockfile_path)
    {
        Ok(lockfile) => lockfile,
        Err(e) => {
            eprintln!(
                "Couldn't open lockfile {0}: {1}",
                door_lockfile_path.display(),
                e
            );
            std::process::exit(1);
        }
    };

    match door_lockfile.try_lock_shared() {
        Ok(_) => (),
        Err(_) => {
            eprintln!("Sorry, {0} is currently undergoing maintenence.", args.door);
            std::process::exit(1);
        }
    };

    let mut node: i8 = 1;
    let mut found_node = false;

    while (node <= door.nodes) && !found_node {
        let node_lockfile_path = rundir.join(format!("{0}.{1}.lock", args.door, node));
        let node_lockfile = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&node_lockfile_path);

        if node_lockfile.is_ok() {
            found_node = match node_lockfile.unwrap().try_lock_exclusive() {
                Ok(_) => true,
                Err(_) => false,
            };
        }

        if found_node {
            let node_rundir = rundir.join(format!("{0}.{1}", args.door, node));

            if node_rundir.exists() {
                match fs::remove_dir_all(&node_rundir) {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Couldn't clean up {0}: {1}", node_rundir.display(), e);
                        std::process::exit(1)
                    }
                };
            }

            let mkdir = fs::create_dir_all(&node_rundir);

            if mkdir.is_err() {
                eprintln!(
                    "Couldn't create node rundir {0}: {1}",
                    node_rundir.display(),
                    mkdir.err().unwrap()
                );
                std::process::exit(1);
            }

            let vars = LaunchVars {
                user: &user,
                node: node,
                current_time: Local::now().format("%H:%M").to_string(),
            };

            let templates = Templates::new();

            match templates.write_dos("door.sys", &node_rundir, &vars) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("{0}", e);
                    std::process::exit(1);
                }
            };

            let commands = match templates.render_string(&door.launch, &vars) {
                Ok(commands) => BatchCommands { commands: commands },
                Err(e) => {
                    eprintln!("Couldn't render launch commands for {0}: {1}", args.door, e);
                    std::process::exit(1);
                }
            };

            match templates.write_dos("doorman.bat", &node_rundir, &commands) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("{0}", e);
                    std::process::exit(1);
                }
            };

            let term = match env::var_os("TERM") {
                Some(val) => match val.into_string() {
                    Ok(val) => val,
                    Err(_) => String::from("xterm"),
                },
                None => String::from("xterm"),
            };

            let run = match Command::new("docker")
                .arg("run")
                .arg("-d")
                .arg("-p5901:5901")
                .arg(format!("-v{0}:/mnt/doorman", node_rundir.display()))
                .arg(format!("-v{0}:/mnt/door", door.path))
                .arg(format!("-eTERM={0}", term))
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
            {
                Ok(run) => run,
                Err(e) => {
                    eprintln!("Couldn't start container: {0}", e);
                    std::process::exit(1);
                }
            };

            let container_id = get_container_id(run);
            println!("Container ID = {0}", container_id.trim());

            match Command::new("docker")
                .arg("exec")
                .arg("-ti")
                .arg(container_id.trim())
                .arg("launch.sh")
                .status()
            {
                Ok(status) => match status.code() {
                    Some(code) => std::process::exit(code),
                    None => {
                        eprintln!("No exit status from client");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("Couldn't start client: {0}", e);
                    std::process::exit(1);
                }
            };
        } else {
            node += 1;
        }
    }

    if !found_node {
        eprintln!("All nodes for {0} are busy!", args.door);
        std::process::exit(1);
    }
}

pub fn configure(args: &SysopCmdArgs, config: &Config) {
    println!("LOL not implemented yet");
    return;
}

pub fn nightly(args: &SysopCmdArgs, config: &Config) {
    println!("LOL not implemented");
    return;
}
