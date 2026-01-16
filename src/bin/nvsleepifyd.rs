use anyhow::Result;
use nvsleepify::daemon;

#[tokio::main]
async fn main() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("Daemon must run as root.");
        std::process::exit(1);
    }
    daemon::run().await
}
