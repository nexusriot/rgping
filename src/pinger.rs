use anyhow::{anyhow, Context, Result};
use std::process::Stdio;
use tokio::{process::Command, time::{sleep, Duration}};

#[derive(Debug, Clone)]
pub struct PingSample {
    pub seq: u64,
    pub rtt_ms: Option<f64>, // None means timeout/loss
}

#[derive(Debug, Clone)]
pub struct PingConfig {
    pub host: String,
    pub interval: Duration,
    pub timeout: Duration,
}

pub struct Pinger {
    cfg: PingConfig,
    seq: u64,
}

impl Pinger {
    pub fn new(cfg: PingConfig) -> Self {
        Self { cfg, seq: 0 }
    }

    async fn ping_once_linux(&mut self) -> Result<PingSample> {
        let timeout_secs = self.cfg.timeout.as_secs().max(1);
        let out = Command::new("ping")
            .arg("-n").arg("-c").arg("1")
            .arg("-w").arg(timeout_secs.to_string())
            .arg(&self.cfg.host)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("failed to execute `ping`")?;

        self.seq += 1;
        let seq = self.seq;

        if !out.status.success() {
            return Ok(PingSample { seq, rtt_ms: None });
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let rtt_ms = stdout
            .lines()
            .find_map(|line| {
                if let Some(idx) = line.find("time=") {
                    let rest = &line[idx + 5..];
                    let end = rest.find(' ').unwrap_or(rest.len());
                    let val = &rest[..end];
                    val.parse::<f64>().ok()
                } else {
                    None
                }
            });

        Ok(PingSample { seq, rtt_ms })
    }

    #[cfg(target_os = "linux")]
    async fn ping_once(&mut self) -> Result<PingSample> {
        self.ping_once_linux().await
    }

    #[cfg(not(target_os = "linux"))]
    async fn ping_once(&mut self) -> Result<PingSample> {
        Err(anyhow!("Non-Linux OS detected: adjust flags in pinger.rs (search for macOS note)."))
    }

    pub async fn run(mut self, mut tx: tokio::sync::mpsc::Sender<PingSample>) -> Result<()> {
        loop {
            let start = tokio::time::Instant::now();
            let sample = self.ping_once().await.unwrap_or_else(|_| {
                self.seq += 1;
                PingSample { seq: self.seq, rtt_ms: None }
            });
            if tx.send(sample).await.is_err() {
                break;
            }
            let elapsed = start.elapsed();
            if elapsed < self.cfg.interval {
                sleep(self.cfg.interval - elapsed).await;
            }
        }
        Ok(())
    }
}