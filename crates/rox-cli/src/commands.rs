//! CLI command definitions using clap.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Rox — Intelligent Nerve System for Robotics
#[derive(Parser, Debug)]
#[command(name = "rox", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create a new Rox project
    New {
        /// Project name
        name: String,
        /// Directory to create in (default: current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// Run Rox with a configuration file
    Run {
        /// Path to config file
        #[arg(short, long, default_value = "config/rox.yml")]
        config: PathBuf,
    },
    /// Monitor a running Rox instance
    Monitor {
        /// API endpoint to connect to
        #[arg(short, long, default_value = "http://localhost:9090")]
        endpoint: String,
    },
    /// Replay a recorded session log
    Replay {
        /// Path to the log file
        #[arg(short, long)]
        log_file: PathBuf,
        /// Replay speed multiplier (1.0 = real-time)
        #[arg(short, long, default_value = "1.0")]
        speed: f64,
    },
}

impl Cli {
    /// Parse CLI arguments.
    pub fn parse_args() -> Self {
        Cli::parse()
    }
}

/// Execute the `new` command — scaffold a Rox project.
pub fn exec_new(name: &str, path: Option<&PathBuf>) -> Result<()> {
    let base = path.cloned().unwrap_or_else(|| PathBuf::from("."));
    let project_dir = base.join(name);

    std::fs::create_dir_all(&project_dir)
        .with_context(|| format!("failed to create directory: {}", project_dir.display()))?;

    // Create Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
rox = {{ version = "0.1" }}
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1"
"#
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;

    // Create src/main.rs
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;

    let main_rs = r#"use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Rox node...");

    // TODO: Initialize your nodes here

    Ok(())
}
"#;
    std::fs::write(src_dir.join("main.rs"), main_rs)?;

    // Create config/rox.yml
    let config_dir = project_dir.join("config");
    std::fs::create_dir_all(&config_dir)?;

    let config_yml = r#"transport:
  tcp:
    bind: "0.0.0.0:7447"

nodes: []
connections: []

agent:
  enabled: false

guard:
  enabled: false
"#;
    std::fs::write(config_dir.join("rox.yml"), config_yml)?;

    tracing::info!(name = name, path = %project_dir.display(), "project created");
    Ok(())
}

/// Execute the `run` command.
pub fn exec_run(config_path: &PathBuf) -> Result<()> {
    let config = rox_core::RoxConfig::from_file(config_path)?;
    let _engine = rox_core::RoxEngine::from_config(config)?;

    tracing::info!(config = %config_path.display(), "rox engine started");
    // Engine lifecycle would be managed by the binary
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_new_project() {
        let dir = tempfile::tempdir().unwrap();
        exec_new("test-robot", Some(&dir.path().to_path_buf())).unwrap();

        assert!(dir.path().join("test-robot/Cargo.toml").exists());
        assert!(dir.path().join("test-robot/src/main.rs").exists());
        assert!(dir.path().join("test-robot/config/rox.yml").exists());
    }

    #[test]
    fn test_new_project_cargo_toml_content() {
        let dir = tempfile::tempdir().unwrap();
        exec_new("my-bot", Some(&dir.path().to_path_buf())).unwrap();

        let content = std::fs::read_to_string(dir.path().join("my-bot/Cargo.toml")).unwrap();
        assert!(content.contains("name = \"my-bot\""));
        assert!(content.contains("rox"));
        assert!(content.contains("tokio"));
    }

    #[test]
    fn test_new_project_default_path() {
        let dir = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        exec_new("default-bot", None).unwrap();
        assert!(dir.path().join("default-bot/Cargo.toml").exists());

        std::env::set_current_dir(original).unwrap();
    }

    #[test]
    fn test_cli_verify() {
        // Verify clap CLI definition is valid
        Cli::command().debug_assert();
    }

    #[test]
    fn test_cli_parse_new() {
        let cli = Cli::try_parse_from(["rox", "new", "my-project"]).unwrap();
        match cli.command {
            Commands::New { name, path } => {
                assert_eq!(name, "my-project");
                assert!(path.is_none());
            }
            _ => panic!("expected New command"),
        }
    }

    #[test]
    fn test_cli_parse_run_default() {
        let cli = Cli::try_parse_from(["rox", "run"]).unwrap();
        match cli.command {
            Commands::Run { config } => {
                assert_eq!(config, PathBuf::from("config/rox.yml"));
            }
            _ => panic!("expected Run command"),
        }
    }

    #[test]
    fn test_cli_parse_replay() {
        let cli =
            Cli::try_parse_from(["rox", "replay", "-l", "session.rlog", "-s", "2.0"]).unwrap();
        match cli.command {
            Commands::Replay { log_file, speed } => {
                assert_eq!(log_file, PathBuf::from("session.rlog"));
                assert!((speed - 2.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected Replay command"),
        }
    }
}
