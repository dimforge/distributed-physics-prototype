#[cfg(feature = "dim2")]
extern crate rapier2d as rapier;
#[cfg(feature = "dim3")]
extern crate rapier3d as rapier;

mod cli;
mod connected_components;
mod neighbors;
mod region_assignment;
mod runner;
mod storage;
mod watch;

use crate::cli::CliArgs;
use crate::storage::{
    start_storage_thread_for_client_objects, start_storage_thread_for_watched_objects,
};
use crate::watch::WatchedObject;
use async_channel::{Receiver, Sender};
use clap::Parser;
use dashmap::DashMap;
use futures::FutureExt;
use log::info;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{JoinHandle, Thread};
use steadyum_api_types::messages::{BodyAssignment, RunnerMessage};
use steadyum_api_types::objects::{ClientBodyObjectSet, WatchedObjects};
use steadyum_api_types::partitionner::SceneUuid;
use steadyum_api_types::region_db::AsyncPartitionnerServer;
use steadyum_api_types::serialization::deserialize;
use steadyum_api_types::simulation::SimulationBounds;
use steadyum_api_types::zenoh::{runner_zenoh_commands_key, ZenohContext};
use tokio::sync::RwLock;
use uuid::Uuid;
use zenoh::config::WhatAmI;
use zenoh::prelude::r#async::AsyncResolve;
use zenoh::prelude::Reliability;
use zenoh::prelude::SplitBuffer;

pub struct RegionThread {
    thread: JoinHandle<anyhow::Result<()>>,
    reg_snd: Sender<RunnerMessage>,
}

pub struct RegionState {
    pub uuid: Uuid, // Used for logs.
    pub app: Arc<AppState>,
    pub reg_rcv: Receiver<RunnerMessage>,
    pub bounds: SimulationBounds,
}

impl RegionState {
    pub fn step_id(&self) -> u64 {
        self.app.step_id.load(Ordering::SeqCst)
    }
}

pub struct AppState {
    pub scene: SceneUuid,
    pub uuid: Uuid,
    pub zenoh: ZenohContext,
    pub regions: DashMap<SimulationBounds, RegionThread>,
    pub step_id: AtomicU64,
    pub main_thread_snd: Sender<RunnerMessage>,
    pub pending_acks: AtomicU64,
    pub main_partitionner: AsyncPartitionnerServer,
    pub local_partitionner: AsyncPartitionnerServer,
    pub static_bodies: RwLock<Vec<BodyAssignment>>,
    pub watch_sets: DashMap<SimulationBounds, WatchedObjects>,
    pub client_object_sets: DashMap<SimulationBounds, ClientBodyObjectSet>,
    pub exit: AtomicBool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(log::LevelFilter::Info);
    builder.init();
    let args = CliArgs::parse();

    let zenoh = ZenohContext::new(
        if args.dev {
            WhatAmI::Peer
        } else {
            WhatAmI::Client
        },
        None,
        false,
    )
    .await?;

    let (main_thread_snd, main_thread_rcv) = async_channel::unbounded();

    let state = Arc::new(AppState {
        scene: SceneUuid(args.typed_scene_uuid()),
        uuid: args.typed_uuid(),
        zenoh,
        regions: DashMap::new(),
        step_id: AtomicU64::new(0),
        main_thread_snd,
        pending_acks: AtomicU64::new(0),
        main_partitionner: AsyncPartitionnerServer::new()?,
        local_partitionner: AsyncPartitionnerServer::local()?,
        static_bodies: RwLock::new(vec![]),
        watch_sets: DashMap::new(),
        client_object_sets: DashMap::new(),
        exit: AtomicBool::new(false),
    });

    start_storage_thread_for_watched_objects(state.clone());
    start_storage_thread_for_client_objects(state.clone());
    main_messages_loop(state, main_thread_rcv).await
}

async fn main_messages_loop(
    state: Arc<AppState>,
    main_thread_rcv: Receiver<RunnerMessage>,
) -> anyhow::Result<()> {
    let runner_zenoh_key = runner_zenoh_commands_key(state.uuid);
    let runner_zenoh_commands_queue = state
        .zenoh
        .session
        .declare_subscriber(&runner_zenoh_key)
        .reliability(Reliability::Reliable)
        .res_async()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut pending_acks = 0;

    loop {
        let message: RunnerMessage = futures::select_biased! {
            message = main_thread_rcv.recv().fuse() => {
                message?
            },
            sample = runner_zenoh_commands_queue.recv_async() => {
                let sample = sample?;
                let payload = sample.value.payload.contiguous();
                deserialize(&payload)?
            }
        };

        match message {
            RunnerMessage::Ack => {
                assert!(pending_acks > 0);
                pending_acks -= 1;

                if pending_acks == 0 {
                    // TODO: hit the main partitionner directly?
                    state.local_partitionner.ack(state.scene).await?;
                }
            }
            RunnerMessage::AssignStaticBodies { mut bodies } => {
                // info!("Adding static bodies: {}", bodies.len());
                state.static_bodies.write().await.append(&mut bodies);
            }
            RunnerMessage::AssignIsland { region, .. } => {
                let region_thread = state
                    .regions
                    .entry(region)
                    .or_insert_with(|| spawn_region(state.clone(), region));
                region_thread.reg_snd.send(message).await?;
            }
            RunnerMessage::Step { step_id } => {
                state.step_id.store(step_id, Ordering::SeqCst);

                pending_acks = state.regions.len();
                for runner in state.regions.iter() {
                    runner.reg_snd.send(RunnerMessage::Step { step_id }).await?;
                }

                // If we don’t have any active runner, ack right away.
                if pending_acks == 0 {
                    // TODO: hit the main partitionner directly?
                    state.local_partitionner.ack(state.scene).await?;
                }
            }
            RunnerMessage::SyncClientObjects => {
                for runner in state.regions.iter() {
                    runner
                        .reg_snd
                        .send(RunnerMessage::SyncClientObjects)
                        .await?;
                }
            }
            RunnerMessage::Exit => {
                state.exit.store(true, Ordering::SeqCst);
                for runner in state.regions.iter() {
                    runner.reg_snd.send(RunnerMessage::Exit).await?;
                }
                break;
            }
        }
    }

    Ok(())
}

fn spawn_region(app: Arc<AppState>, region: SimulationBounds) -> RegionThread {
    let (reg_snd, reg_rcv) = async_channel::unbounded();
    let uuid = Uuid::new_v4();
    let reg_state = RegionState {
        uuid,
        app,
        reg_rcv,
        bounds: region,
    };

    info!("Spawning thread {:?} for region {:?}.", uuid, region);

    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(runner::run_simulation(reg_state))
    });

    RegionThread { thread, reg_snd }
}
