mod pinger;
mod ui;

use std::time::Duration;

use anyhow::Result;
use clap::{Parser, ValueHint};
use tokio::signal;
use tokio::sync::mpsc;

use pinger::{Pinger, PingConfig};
use ui::{Ui, UiConfig};

#[derive(Parser, Debug)]
#[command(name = "rgping", version, about = "Minimal gping-like live latency graph in your terminal")]
struct Args {
    #[arg(value_hint = ValueHint::Hostname)]
    host: String,

    #[arg(short = 'i', long, default_value_t = 1000)]
    interval_ms: u64,

    #[arg(short = 't', long, default_value_t = 1000)]
    timeout_ms: u64,

    #[arg(short = 'H', long, default_value_t = 120)]
    history: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let (tx, rx) = mpsc::channel::<pinger::PingSample>(256);

    let pinger_cfg = PingConfig {
        host: args.host.clone(),
        interval: Duration::from_millis(args.interval_ms),
        timeout: Duration::from_millis(args.timeout_ms),
    };
    let pinger = Pinger::new(pinger_cfg);

    let ping_task = tokio::spawn(async move {
        let _ = pinger.run(tx).await;
    });

    let ui = Ui::new(UiConfig {
        host: args.host.clone(),
        history: args.history,
    });

    let ui_task = tokio::task::spawn_blocking(move || ui.run_tui(rx));

    tokio::select! {
        _ = signal::ctrl_c() => {},
        _ = ping_task => {},
        ui_res = ui_task => {
            ui_res??;
        }
    }

    Ok(())
}
