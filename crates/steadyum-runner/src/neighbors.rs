use async_channel::Sender;
use futures::{stream, StreamExt, TryStreamExt};
use std::collections::{hash_map::Entry, HashMap};
use std::time::Duration;
use steadyum_api_types::messages::{RunnerMessage, PARTITIONNER_QUEUE};
use steadyum_api_types::partitionner::SceneUuid;
use steadyum_api_types::region_db::AsyncPartitionnerServer;
use steadyum_api_types::simulation::SimulationBounds;
use steadyum_api_types::zenoh::{runner_zenoh_commands_key, ZenohContext};
use uuid::Uuid;
use zenoh::prelude::r#async::*;
// use zenoh::prelude::SplitBuffer;
use zenoh::publication::Publisher;
// use zenoh::sample::Sample;
use crate::AppState;
use steadyum_api_types::serialization::{deserialize, serialize};
use zenoh::subscriber::Subscriber;

pub enum NeighborRunner<'a> {
    Local { sender: Sender<RunnerMessage> },
    Remote { queue: Publisher<'a>, uuid: Uuid },
}

impl<'a> NeighborRunner<'a> {
    pub async fn send(&self, message: &RunnerMessage) -> anyhow::Result<()> {
        match self {
            Self::Local { sender } => Ok(sender.send(message.clone()).await?),
            Self::Remote { queue, .. } => {
                let data = serialize(message)?;
                Ok(queue
                    .put(data)
                    .res()
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?)
            }
        }
    }
}

pub struct Neighbors<'a> {
    pub zenoh: &'a ZenohContext,
    pub runners: HashMap<SimulationBounds, NeighborRunner<'a>>,
}

impl<'a> Neighbors<'a> {
    pub fn new(zenoh: &'a ZenohContext) -> Self {
        Self {
            zenoh,
            runners: HashMap::default(),
        }
    }

    pub async fn spawn_neighbors(
        &mut self,
        app: &AppState,
        db: &AsyncPartitionnerServer,
        scene: SceneUuid,
        regions: impl Iterator<Item = SimulationBounds>,
    ) {
        for region in regions {
            match self.runners.entry(region) {
                Entry::Vacant(entry) => {
                    if let Some(runner) = app.regions.get(&region) {
                        // The region already exists locally.
                        entry.insert(NeighborRunner::Local {
                            sender: app.main_thread_snd.clone(),
                        });
                        continue;
                    }

                    let uuid = db.allocate_runner(scene, region).await.unwrap();

                    if uuid == app.uuid {
                        // The region will be local to this process.
                        entry.insert(NeighborRunner::Local {
                            sender: app.main_thread_snd.clone(),
                        });
                    } else {
                        // The region exists on another node.
                        let zenoh_key = runner_zenoh_commands_key(uuid);
                        let queue = self
                            .zenoh
                            .session
                            .declare_publisher(zenoh_key)
                            .congestion_control(CongestionControl::Block)
                            .res()
                            .await
                            .unwrap();
                        entry.insert(NeighborRunner::Remote { queue, uuid });
                    }
                }
                Entry::Occupied(_) => {}
            }
        }
    }

    pub fn fetch_neighbor(&self, region: SimulationBounds) -> &NeighborRunner<'a> {
        &self.runners[&region]
    }
}
