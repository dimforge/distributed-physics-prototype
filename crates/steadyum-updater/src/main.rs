use log::{error, info, warn};
use std::process::{Child, Command};
use std::time::Duration;
use steadyum_api_types::region_db::AsyncPartitionnerServer;

#[derive(Default)]
struct AppState {
    partitionner: Option<Child>,
}

impl AppState {
    async fn kill_partitionner(&mut self) -> anyhow::Result<()> {
        if let Some(mut p) = self.partitionner.take() {
            let local_partitionner = AsyncPartitionnerServer::local()?;
            if let Err(e) = local_partitionner.shutdown().await {
                error!("Shutdown query returned error status: {e}");
            }

            if let Err(e) = p.wait() {
                error!("Failed to wait for partitionner shutdown: {e}");
            } else {
                info!("Successfully shutdown partitionner.");
            }
        }

        Ok(())
    }

    fn spawn_partitionner(&mut self) {
        assert!(self.partitionner.is_none());
        let mut args = vec!["--runner".to_string()];
        match Command::new("./partitionner").args(args).spawn() {
            Ok(child) => self.partitionner = Some(child),
            Err(e) => error!("Could not start partitionner: {}", e),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_log();

    let mut state = AppState::default();
    let main_partitionner = AsyncPartitionnerServer::new()?;

    loop {
        // Send heartbeat.
        match main_partitionner.heartbeat().await {
            Ok(()) => {
                if state.partitionner.is_none() {
                    // Request the partitionner+runner executables.
                    match main_partitionner.get_exes().await {
                        Ok(exes) => {
                            info!("Retireving runner/partitionner executables.");
                            tokio::fs::write("partitionner", exes.partitionner)
                                .await
                                .unwrap();
                            tokio::fs::write("runner", exes.runner).await.unwrap();

                            #[cfg(target_family = "unix")]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                tokio::fs::set_permissions(
                                    "partitionner",
                                    std::fs::Permissions::from_mode(755),
                                )
                                .await
                                .unwrap();
                                tokio::fs::set_permissions(
                                    "runner",
                                    std::fs::Permissions::from_mode(755),
                                )
                                .await
                                .unwrap();
                            }

                            info!("Retireved runners/partitioners. Spawning partitionner.");
                            state.spawn_partitionner();
                        }
                        Err(e) => error!("Couldn’t retrieve executables: {e}"),
                    }
                }
            }
            Err(e) => {
                // Partitionner didn’t respond, kill the local instance.
                // NOTE: this is probably too aggressive for any production usage,
                //       but good enough to make the testing experience better.
                warn!("Couldn’t reach partitionner: {e}. Stopping node.");
                // NOTE: right now the partitionner exists before having a chance to
                //       answer the query.
                if let Err(e) = state.kill_partitionner().await {
                    eprintln!("Error stopping partitionner: {e}");
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn init_log() {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(log::LevelFilter::Info);
    builder.init();
}
