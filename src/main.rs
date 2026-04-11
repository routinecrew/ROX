use anyhow::Result;
use clap::Parser;
use tokio::sync::broadcast;
#[cfg(feature = "agent")]
use tokio::sync::mpsc;
use tracing::info;

use rox_cli::{Cli, Commands};
use rox_core::{RoxConfig, RoxEngine};
use rox_protocol::AgentEvent;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::New { name, path } => {
            rox_cli::exec_new(&name, path.as_ref())?;
            println!("Created new Rox project: {name}");
        }

        Commands::Run { config } => {
            info!("Rox -- Intelligent Nerve System for Robotics");
            info!(config = %config.display(), "loading configuration");

            let rox_config = RoxConfig::from_file(&config)?;

            let (event_tx, _) = broadcast::channel::<AgentEvent>(256);

            // Start API server if configured
            if let Some(api_config) = &rox_config.api {
                if api_config.enabled {
                    let api = rox_api::ApiServer::new(
                        &api_config.bind,
                        rox_config.nodes.clone(),
                        event_tx.clone(),
                    );
                    tokio::spawn(async move {
                        if let Err(e) = api.serve().await {
                            tracing::error!(error = %e, "API server error");
                        }
                    });
                }
            }

            // Initialize engine
            let mut engine = RoxEngine::from_config(rox_config)?;

            // Start Agent runtime if configured
            #[cfg(feature = "agent")]
            {
                let (metrics_tx, metrics_rx) = mpsc::channel(1024);
                let policy_tx = engine.policy_sender();
                let mut agent_runtime = rox_agent::AgentRuntime::new(metrics_rx, policy_tx)
                    .with_event_broadcast(event_tx.clone());
                tokio::spawn(async move {
                    agent_runtime.run().await;
                });
            }

            info!("rox engine running (press Ctrl+C to stop)");

            // Main loop
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(10));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        engine.tick().await?;
                    }
                    _ = tokio::signal::ctrl_c() => {
                        info!("shutting down...");
                        engine.shutdown().await?;
                        break;
                    }
                }
            }
        }

        Commands::Monitor { endpoint } => {
            info!(endpoint = %endpoint, "connecting to Rox instance...");
            println!("Monitoring {endpoint} (not yet implemented)");
        }

        Commands::Replay { log_file, speed } => {
            info!(file = %log_file.display(), speed = speed, "replaying session");

            let mut replay = rox_replay::ReplayEngine::open(&log_file)?;
            replay.load()?;

            let clock = if (speed - 1.0).abs() < f64::EPSILON {
                rox_replay::ReplayClock::OriginalTiming
            } else {
                rox_replay::ReplayClock::Scaled(speed)
            };

            let replay = replay.with_clock(clock);

            let stats = replay.stats().clone();
            info!(
                cycles = stats.total_cycles,
                messages = stats.total_messages,
                policies = stats.total_policies,
                "loaded session"
            );

            replay
                .replay(
                    |cycle, msg| {
                        tracing::debug!(
                            cycle,
                            key = msg.header.key.as_str(),
                            seq = msg.header.sequence,
                            "replay message"
                        );
                        Ok(())
                    },
                    |cycle, policy| {
                        tracing::debug!(cycle, version = policy.version, "replay policy");
                        Ok(())
                    },
                )
                .await?;

            println!("Replay complete.");
        }
    }

    Ok(())
}
