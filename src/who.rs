use super::cfg::Config;
use super::{OutputFormat, WhoArgs};
use anyhow::{anyhow, Result};
use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use chrono_humanize::{Accuracy, HumanTime, Tense};
use comfy_table::modifiers::{UTF8_ROUND_CORNERS, UTF8_SOLID_INNER_BORDERS};
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Table};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize, Debug)]
struct DockerPS {
    #[serde(rename = "ID")]
    container_id: String,

    #[serde(rename = "CreatedAt")]
    created_str: String,

    #[serde(rename = "Labels")]
    labels: String,
}

#[derive(Deserialize, Debug)]
struct PodmanPS {
    #[serde(rename = "Id")]
    container_id: String,

    #[serde(rename = "Created")]
    created_ts: i64,

    #[serde(rename = "Labels")]
    labels: Option<HashMap<String, String>>,
}

#[derive(Serialize, Debug)]
pub struct WhoNode {
    pub container_id: String,
    pub user: String,
    pub door: String,
    pub node: Option<i8>,
    pub command: Option<String>,

    #[serde(with = "ts_seconds")]
    pub since: DateTime<Utc>,
}

fn split_docker_label(label: &str) -> (&str, &str) {
    let mut splitter = label.splitn(2, "=");

    let key = splitter.next().unwrap();

    if let Some(value) = splitter.next() {
        return (key, value);
    }

    (key, "")
}

fn parse_docker_line(line: &str) -> Result<WhoNode> {
    let parsed: DockerPS = serde_json::from_str(line)?;

    let mut user: Option<String> = None;
    let mut door: Option<String> = None;
    let mut node: Option<i8> = None;
    let mut command: Option<String> = None;

    for label in parsed.labels.split(",") {
        let (key, value) = split_docker_label(label);
        match key {
            "doorman.user" => user = Some(String::from(value)),
            "doorman.door" => door = Some(String::from(value)),
            "doorman.node" => node = Some(value.parse::<i8>().unwrap()),
            "doorman.command" => command = Some(String::from(value)),
            _ => (),
        }
    }

    let since: DateTime<Utc> = DateTime::parse_from_str(&parsed.created_str, "%F %T %z %Z")?.into();

    if let (Some(user), Some(door)) = (user, door) {
        Ok(WhoNode {
            container_id: parsed.container_id,
            user,
            door,
            node,
            command,
            since,
        })
    } else {
        Err(anyhow!("Couldn't parse line"))
    }
}

fn parse_docker(output: &str) -> Vec<WhoNode> {
    let mut nodes: Vec<WhoNode> = vec![];

    for line in output.lines() {
        if let Ok(node) = parse_docker_line(line) {
            nodes.push(node);
        }
    }

    nodes
}

fn parse_podman_container(container: &PodmanPS) -> Result<WhoNode> {
    let labels = container.labels.clone().unwrap();
    let user = labels.get("doorman.user");
    let door = labels.get("doorman.door");
    let node = labels.get("doorman.node");
    let command = labels.get("doorman.command");
    let since = DateTime::from_timestamp(container.created_ts, 0).unwrap();

    if let (Some(user), Some(door)) = (user, door) {
        Ok(WhoNode {
            container_id: container.container_id.clone(),
            user: user.clone(),
            door: door.clone(),
            node: node.map(|value| value.parse::<i8>().unwrap()),
            command: command.map(|value| value.clone()),
            since,
        })
    } else {
        Err(anyhow!("Couldn't parse podman container"))
    }
}

fn parse_podman(output: &str) -> Result<Vec<WhoNode>> {
    let containers: Vec<PodmanPS> = serde_json::from_str(output)?;
    let mut nodes: Vec<WhoNode> = vec![];

    for container in containers {
        if let Ok(node) = parse_podman_container(&container) {
            nodes.push(node);
        }
    }

    Ok(nodes)
}

fn parse_ps(output: &str) -> Vec<WhoNode> {
    if let Ok(nodes) = parse_podman(output) {
        nodes
    } else {
        parse_docker(output)
    }
}

fn who(container_engine: &PathBuf, door: &Option<String>) -> Result<Vec<WhoNode>> {
    let mut ps = Command::new(container_engine);

    ps.arg("ps")
        .arg("--format=json")
        .arg("--filter")
        .arg(door.clone().map_or_else(
            || "label=doorman.door".to_string(),
            |door| format!("label=doorman.door={}", door),
        ));

    let output = ps.output()?;

    if let Some(code) = output.status.code() {
        if code != 0 {
            return Err(anyhow!("'docker ps' exited with non-zero status: {}", code));
        }
    } else {
        return Err(anyhow!("'docker ps' exited with unknown status"));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut nodes = parse_ps(&stdout);

    nodes.sort_by(|a, b| match a.door.cmp(&b.door) {
        Ordering::Equal => a.node.unwrap_or(0).cmp(&b.node.unwrap_or(0)),
        other => other,
    });

    Ok(nodes)
}

fn print_who(format: &Option<OutputFormat>, nodes: &Vec<WhoNode>) -> Result<()> {
    if let Some(format) = format {
        println!(
            "{}",
            match format {
                OutputFormat::JSON => serde_json::to_string(&nodes)?,
                OutputFormat::YAML => serde_yaml::to_string(&nodes)?,
            }
        );

        return Ok(());
    }

    if nodes.is_empty() {
        println!("Nobody is playing anything right now. How boring.");
        return Ok(());
    }

    let mut table = Table::new();

    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_header(vec!["User", "Door", "Node", "Duration"]);

    for node in nodes {
        let duration = HumanTime::from(Utc::now().signed_duration_since(node.since));

        table.add_row(vec![
            Cell::new(&node.user),
            Cell::new(&node.door),
            Cell::new(node.node.map_or_else(
                || node.command.clone().unwrap_or("???".to_string()),
                |i| i.to_string(),
            )),
            Cell::new(duration.to_text_en(Accuracy::Rough, Tense::Present)),
        ]);
    }

    println!("{table}");

    Ok(())
}

pub fn who_command(args: &WhoArgs, config: &Config) -> Result<()> {
    let container_engine = config.container_engine()?;
    let nodes = who(&container_engine.path, &args.door)?;

    print_who(&args.format, &nodes)?;

    Ok(())
}
