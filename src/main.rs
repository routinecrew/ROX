use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("Rox — Intelligent Nerve System for Robotics");
    tracing::info!("Use `rox --help` for CLI options");
    Ok(())
}
