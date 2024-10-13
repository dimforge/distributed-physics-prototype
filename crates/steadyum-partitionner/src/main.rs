mod cli;
mod storage;

#[macro_use]
extern crate dotenv_codegen;

use crate::cli::CliArgs;
use crate::storage::start_storage_thread;
use async_channel::{Receiver, Sender};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{routing::post, Json, Router};
use clap::Parser;
use log::{error, info};
use std::collections::{HashMap, HashSet, VecDeque};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar};
use std::time::Duration;
use steadyum_api_types::env::CONFIG;
use steadyum_api_types::messages::{BodyAssignment, RunnerMessage};
use steadyum_api_types::objects::{ClientBodyObjectSet, RegionList, SceneList, WatchedObjects};
use steadyum_api_types::partitionner::{
    AckRequest, AssignRunnerRequest, AssignRunnerResponse, ChildPartitionner, ClientInputRequest,
    CreateSceneRequest, CreateSceneResponse, GetExesResponse, InsertObjectsRequest,
    ListRegionsRequest, RegisterChildRequest, RemoveSceneRequest, RunnerInitializedRequest,
    SceneUuid, StartStopRequest, StepRequest, ACK_ENDPOINT, ASSIGN_RUNNER_ENDPOINT,
    CLIENT_INPUT_ENDPOINT, CREATE_SCENE_ENDPOINT, GET_EXES, HEARTBEAT, INSERT_OBJECTS_ENDPOINT,
    LIST_REGIONS_ENDPOINT, LIST_SCENES_ENDPOINT, NUM_INTERNAL_STEPS, REGISTER_CHILD_ENDPOINT,
    REMOVE_SCENE_ENDPOINT, RUNNER_INITIALIZED_ENDPOINT, SHUTDOWN, START_STOP_ENDPOINT,
    STEP_ENDPOINT,
};
use steadyum_api_types::rapier::parry::bounding_volume::{Aabb, BoundingVolume};
use steadyum_api_types::rapier::parry::query::PointQuery;
use steadyum_api_types::rapier::parry::shape::Cuboid;
use steadyum_api_types::region_db::AsyncPartitionnerServer;
use steadyum_api_types::serialization::serialize;
use steadyum_api_types::simulation::SimulationBounds;
use steadyum_api_types::zenoh::{runner_zenoh_commands_key, ZenohContext};
use tokio::sync::{Mutex, OnceCell, RwLock};
use tokio::time::Instant;
use uuid::Uuid;
use zenoh::config::WhatAmI;
use zenoh::prelude::r#async::AsyncResolve;
use zenoh::prelude::CongestionControl;
use zenoh::publication::Publisher;

const MAX_PENDING_RUNNERS: u32 = 10;

// static ZENOH: OnceCell<ZenohContext> = OnceCell::const_new();
//
// async fn init_zenoh(my_type: PartitionnerType) -> &'static ZenohContext {
//     ZENOH.get_or_init(|| async {
//         if my_type == PartitionnerType::Dev {
//             ZenohContext::new(WhatAmI::Peer, None).await.unwrap()
//         } else {
//             ZenohContext::new(WhatAmI::Router, None).await.unwrap()
//         }
//     })
// }
//
// fn zenoh() -> &'static ZenohContext {
//     ZENOH.get().unwrap()
// }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PartitionnerType {
    Master,
    Runner,
    Dev,
}

pub struct Runner {
    pub process: Option<Child>,
    pub uuid: Uuid,
    pub is_new: bool,
}

pub struct LiveRunners {
    pub next_port_id: u32,
    pub exited: HashSet<SceneUuid>,
    pub assigned: HashMap<(SceneUuid, SimulationBounds), Uuid>,
    pub per_node: HashMap<SceneUuid, Vec<Runner>>,
    pub to_remove: Sender<Child>,
}

impl LiveRunners {
    fn default(to_remove: Sender<Child>) -> Self {
        Self {
            exited: HashSet::default(),
            next_port_id: 10_000,
            assigned: HashMap::default(),
            per_node: HashMap::default(),
            to_remove,
        }
    }
}

struct SceneGeometry {
    // TODO: this doesn’t support children added dynamically
    //       after the scene is created.
    children_bounds: Vec<Aabb>,
}

struct SceneAcks {
    pending_acks: AtomicIsize,
    step_id: AtomicU64,
    step_limit: AtomicU64,
    date: RwLock<Instant>,
}

impl Default for SceneAcks {
    fn default() -> Self {
        Self {
            pending_acks: Default::default(),
            step_id: Default::default(),
            step_limit: Default::default(),
            date: RwLock::new(Instant::now()),
        }
    }
}

struct SharedState {
    runners: Mutex<LiveRunners>,
    zenoh: ZenohContext,
    running: AtomicBool,
    my_type: PartitionnerType,
    children: Mutex<Vec<AsyncPartitionnerServer>>,
    next_child: AtomicUsize,
    scenes_acks: RwLock<HashMap<SceneUuid, SceneAcks>>,
    scenes_geometries: RwLock<HashMap<SceneUuid, SceneGeometry>>,
    static_bodies: RwLock<HashMap<SceneUuid, Vec<BodyAssignment>>>,
    parent_partitionner: Option<AsyncPartitionnerServer>,
    assign_runner_lock: Mutex<()>,
    inputs_snd: Sender<ClientInputRequest>,
    inputs_rcv: Receiver<ClientInputRequest>,
}

#[derive(Clone)]
pub struct AppState {
    data: Arc<SharedState>,
}

impl AppState {
    pub async fn with_type(my_type: PartitionnerType, to_remove: Sender<Child>) -> Self {
        let (inputs_snd, inputs_rcv) = async_channel::unbounded();
        Self {
            data: Arc::new(SharedState {
                my_type,
                zenoh: if my_type == PartitionnerType::Dev {
                    ZenohContext::new(WhatAmI::Peer, None, true).await.unwrap()
                } else {
                    ZenohContext::new(WhatAmI::Router, None, true)
                        .await
                        .unwrap()
                },
                runners: Mutex::new(LiveRunners::default(to_remove)),
                running: AtomicBool::new(false),
                children: Mutex::new(vec![]),
                next_child: AtomicUsize::new(0),
                scenes_acks: RwLock::new(HashMap::new()),
                scenes_geometries: RwLock::new(HashMap::new()),
                static_bodies: RwLock::new(HashMap::new()),
                assign_runner_lock: Mutex::new(()),
                inputs_snd,
                inputs_rcv,
                parent_partitionner: if my_type == PartitionnerType::Runner {
                    Some(AsyncPartitionnerServer::new().unwrap())
                } else {
                    None
                },
            }),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_log();

    let args = CliArgs::parse();

    let my_type = if args.dev {
        PartitionnerType::Dev
    } else if args.runner {
        PartitionnerType::Runner
    } else {
        PartitionnerType::Master
    };

    info!("Running partitionner as: {:?}", my_type);

    let (to_remove_snd, to_remove_rcv) = async_channel::unbounded();
    let mut state = AppState::with_type(my_type, to_remove_snd).await;
    let state_clone2 = state.clone();

    if my_type == PartitionnerType::Runner {
        // Register this partitionner in the parent.
        let network_interfaces = local_ip_address::list_afinet_netifas()?;
        let my_local_ip = network_interfaces
            .iter()
            .find(|int| int.0 == CONFIG.priv_net_int)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine local IP address: private network interface not found."
                )
            })
            .map(|(_, ip)| ip)?;

        info!("My local ip: {:?}", my_local_ip);
        let parent_server = AsyncPartitionnerServer::new().unwrap();
        let me = ChildPartitionner {
            addr: format!("http://{my_local_ip}"),
            port: CONFIG.partitionner_port,
        };
        parent_server.register_child(me.clone()).await?;
    }

    if my_type != PartitionnerType::Master {
        std::thread::spawn(move || {
            smol::block_on(runner_init_validation_loop(state_clone2));
        });

        std::thread::spawn(move || runner_stopped_child_wait_loop(to_remove_rcv));
    }

    if my_type != PartitionnerType::Runner {
        input_handling_loop(state.clone());
    }

    // if my_type != PartitionnerType::Runner {
    //     start_storage_thread(state.clone());
    // }

    let app = Router::new()
        .route(SHUTDOWN, get(shutdown))
        .route(HEARTBEAT, get(heartbeat))
        .route(GET_EXES, get(get_exes))
        .route(ASSIGN_RUNNER_ENDPOINT, post(assign_runner))
        .route(RUNNER_INITIALIZED_ENDPOINT, post(runner_initialized))
        .route(INSERT_OBJECTS_ENDPOINT, post(insert_objects))
        .route(LIST_REGIONS_ENDPOINT, get(list_regions))
        .route(LIST_SCENES_ENDPOINT, get(list_scenes))
        .route(START_STOP_ENDPOINT, post(start_stop))
        .route(REGISTER_CHILD_ENDPOINT, post(register_child))
        .route(CREATE_SCENE_ENDPOINT, post(create_scene))
        .route(REMOVE_SCENE_ENDPOINT, post(remove_scene))
        .route(ACK_ENDPOINT, post(ack))
        .route(STEP_ENDPOINT, post(step))
        .route(CLIENT_INPUT_ENDPOINT, post(handle_client_inputs))
        .with_state(state);
    axum::Server::bind(
        &format!("0.0.0.0:{}", CONFIG.partitionner_port)
            .parse()
            .unwrap(),
    )
    .serve(app.into_make_service())
    .await?;

    Ok(())
}

async fn handle_client_inputs(
    State(state): State<AppState>,
    Json(payload): Json<ClientInputRequest>,
) {
    // info!("Got clinet input.");
    state.data.inputs_snd.send(payload).await.unwrap();
}

async fn step(State(state): State<AppState>, Json(payload): Json<StepRequest>) {
    if state.data.my_type != PartitionnerType::Runner && !state.data.running.load(Ordering::SeqCst)
    {
        info!("Could not step {:?}: simulation paused.", payload.scene);
        return; // Can’t step if we are not running the simulation.
    }

    info!(
        "Stepping {:?} with step id: {}.",
        payload.scene, payload.step_id
    );

    let scenes_acks = state.data.scenes_acks.read().await;

    if let Some(scene_acks) = scenes_acks.get(&payload.scene) {
        // Print timing info.

        {
            let new_date = Instant::now();
            let mut scene_date = scene_acks.date.write().await;
            let duration = new_date.duration_since(*scene_date);
            info!(
                "[{:?}] Time since last stepping: {}",
                payload.scene,
                duration.as_secs_f32()
            );
            *scene_date = new_date;
        }

        // We are a leaf instance, step the runners associated to this scene.
        match state.data.my_type {
            PartitionnerType::Master => {
                // We are the master instance, send a step query to all child partitionner.
                let children_to_notify: Vec<_> = {
                    let children = state.data.children.lock().await;
                    children.iter().cloned().collect()
                };

                scene_acks
                    .pending_acks
                    .store(children_to_notify.len() as isize, Ordering::SeqCst);
                scene_acks.step_id.store(payload.step_id, Ordering::SeqCst);

                for child_partitionner in children_to_notify {
                    child_partitionner
                        .step(payload.scene, payload.step_id)
                        .await
                        .unwrap();
                }
            }
            PartitionnerType::Runner | PartitionnerType::Dev => {
                let runners_to_notify: Vec<_> = {
                    let runners = state.data.runners.lock().await;
                    runners
                        .per_node
                        .iter()
                        .filter(|(scene, _)| **scene == payload.scene)
                        .flat_map(|(_, r)| r.iter().map(|r| r.uuid))
                        .collect()
                };

                scene_acks
                    .pending_acks
                    .store(runners_to_notify.len() as isize, Ordering::SeqCst);
                scene_acks.step_id.store(payload.step_id, Ordering::SeqCst);

                info!("Stepping {} runners.", runners_to_notify.len());

                if runners_to_notify.is_empty() {
                    // This child partitionner doesn’t have any active runner
                    // for this scene. Ack immediately.
                    if let Some(parent_partitionner) = &state.data.parent_partitionner {
                        info!("No runner to wait on, acking the parent partitionner.");
                        parent_partitionner.ack(payload.scene).await.unwrap();
                    }
                }

                for uuid in runners_to_notify {
                    put_runner_message(
                        &state.data.zenoh,
                        uuid,
                        RunnerMessage::Step {
                            step_id: payload.step_id,
                        },
                    )
                    .await
                    .unwrap();
                }
            }
        }
    }
}

async fn ack(State(state): State<AppState>, Json(payload): Json<AckRequest>) {
    let scenes_acks = state.data.scenes_acks.read().await;
    if let Some(scene_acks) = scenes_acks.get(&payload.scene) {
        let val_before = scene_acks.pending_acks.fetch_add(-1, Ordering::SeqCst);

        info!("Received ack, remaining: {}", val_before - 1);

        // NOTE: if the value was 1, it’s now 0.
        if val_before <= 1 {
            // All the children acked for this scene.
            // Notify the parent if we have one.
            match state.data.my_type {
                PartitionnerType::Master => {
                    let new_step_id = scene_acks.step_id.fetch_add(1, Ordering::SeqCst) + 1;
                    if new_step_id <= scene_acks.step_limit.load(Ordering::SeqCst) {
                        step(
                            State(state.clone()),
                            Json(StepRequest {
                                scene: payload.scene,
                                step_id: new_step_id,
                            }),
                        )
                        .await
                    } else {
                        state.data.running.store(false, Ordering::SeqCst);
                    }
                }
                PartitionnerType::Runner => {
                    // We are a leaf instance, send an ack to the parent partitionner.
                    let parent_server = AsyncPartitionnerServer::new().unwrap();
                    parent_server.ack(payload.scene).await.unwrap();
                }
                PartitionnerType::Dev => {
                    let new_step_id = scene_acks.step_id.fetch_add(1, Ordering::SeqCst) + 1;
                    if new_step_id <= scene_acks.step_limit.load(Ordering::SeqCst) {
                        step(
                            State(state.clone()),
                            Json(StepRequest {
                                scene: payload.scene,
                                step_id: new_step_id,
                            }),
                        )
                        .await
                    } else {
                        println!("################# Stopping runners ################");
                        state.data.running.store(false, Ordering::SeqCst);
                    }
                }
            }
        }
    }
}

async fn shutdown(State(state): State<AppState>) {
    let mut runners = state.data.runners.lock().await;
    let runners = &mut *runners;
    for runner in runners.per_node.values_mut().flat_map(|r| r.iter_mut()) {
        if let Some(proc) = &mut runner.process {
            if let Err(e) = proc.kill() {
                error!("Failed to stop child runner.");
            }
            if let Err(e) = proc.wait() {
                error!("Failed to wait for child runner termination.");
            }
        }
    }
    std::process::abort();
}

async fn get_exes() -> bytes::Bytes {
    info!("Getting exes.");
    let runner = tokio::fs::read(&CONFIG.runner_exe)
        .await
        .expect("Failed to read runner exe");
    let partitionner = tokio::fs::read("steadyum-partitionner")
        .await
        .expect("Failed to read partitionner exe");
    let resp = GetExesResponse {
        partitionner,
        runner,
    };

    let result = serialize(&resp).unwrap();
    result.into()
}

async fn heartbeat() {}

async fn create_scene(
    State(state): State<AppState>,
    Json(payload): Json<CreateSceneRequest>,
) -> Json<CreateSceneResponse> {
    fn split_aabb(aabb: Aabb) -> [Aabb; 2] {
        let extents = aabb.extents();
        let center = aabb.center();
        let split_axis = extents.xz().imax() * 2;
        let mut left = aabb;
        let mut right = aabb;
        left.maxs[split_axis] = center[split_axis];
        right.mins[split_axis] = center[split_axis];
        [left, right]
    }

    fn subdivide_domain(domain: Aabb, num_subdivs: usize) -> Vec<Aabb> {
        let mut subdivisions = VecDeque::new();
        subdivisions.push_back(domain);
        for _ in 1..num_subdivs {
            let to_sub = subdivisions.pop_front().unwrap();
            let subs = split_aabb(to_sub);
            subdivisions.push_back(subs[0]);
            subdivisions.push_back(subs[1]);
        }

        subdivisions.into()
    }

    info!(
        "Creating scene {:?} with bounds {:?}.",
        payload.scene, payload.bounds
    );

    let num_child_partitioners = state.data.children.lock().await.len();
    let children_bounds = subdivide_domain(payload.bounds, num_child_partitioners);
    let scene_geom = SceneGeometry {
        children_bounds: children_bounds.clone(),
    };

    state
        .data
        .scenes_geometries
        .write()
        .await
        .insert(payload.scene, scene_geom);
    state
        .data
        .static_bodies
        .write()
        .await
        .insert(payload.scene, vec![]);
    state
        .data
        .scenes_acks
        .write()
        .await
        .insert(payload.scene, SceneAcks::default());

    let response = match state.data.my_type {
        PartitionnerType::Master => {
            let mut runners_per_node = vec![];
            let children_to_notify: Vec<_> = {
                let children = state.data.children.lock().await;
                children.iter().cloned().collect()
            };

            for (child_partitionner, child_bounds) in
                children_to_notify.iter().zip(children_bounds.iter())
            {
                let response = child_partitionner
                    .create_scene(payload.scene, *child_bounds)
                    .await
                    .unwrap();
                runners_per_node.push(Runner {
                    process: None,
                    uuid: response.runner,
                    is_new: true,
                });
            }

            let mut locked_runners = state.data.runners.lock().await;
            locked_runners
                .per_node
                .insert(payload.scene, runners_per_node);

            // Response doesn’t matter for the master partitionner.
            CreateSceneResponse {
                runner: Uuid::new_v4(),
            }
        }
        _ => {
            // Spawn the runner and wait for it to become ready.
            // NOTE: creating a scene is a one-time event, so it sounds acceptable for it to
            //       take a bit of time (instead of having pre-spawned runners).
            let uuid = Uuid::new_v4();
            log::info!(
                "Spawning new runner: {:?}, path : {}.",
                uuid,
                CONFIG.runner_exe
            );
            let mut locked_runners = state.data.runners.lock().await;
            let mut args = vec![
                "--uuid".to_string(),
                format!("{}", uuid.to_u128_le()),
                "--scene-uuid".to_string(),
                format!("{}", payload.scene.0.to_u128_le()),
            ];

            if state.data.my_type == PartitionnerType::Dev {
                args.push("--dev".to_string());
            }

            let process = Command::new(&CONFIG.runner_exe).args(args).spawn().unwrap();
            let runner = Runner {
                process: Some(process),
                uuid,
                is_new: true,
            };

            // FIXME: wait for the runner to be ready.
            tokio::time::sleep(Duration::from_secs(1)).await;

            locked_runners.per_node.insert(payload.scene, vec![runner]);

            CreateSceneResponse { runner: uuid }
        }
    };

    info!("Done creating scene {:?}", payload.scene);
    Json(response)
}

async fn remove_scene(State(state): State<AppState>, Json(payload): Json<RemoveSceneRequest>) {
    info!("Removing scene: {:?}", payload.scene.0);
    let mut runners = state.data.runners.lock().await;
    let runners = &mut *runners;
    runners.exited.insert(payload.scene);

    if let Some(node_runners) = runners.per_node.remove(&payload.scene) {
        for mut runner in node_runners {
            info!("Exiting runner: {:?}", runner.uuid);

            put_runner_message(&state.data.zenoh, runner.uuid, RunnerMessage::Exit)
                .await
                .unwrap();

            if let Some(child) = runner.process.take() {
                runners.to_remove.send(child).await.unwrap();
            }
        }
    }
    runners
        .assigned
        .retain(|(scene, _), _| *scene != payload.scene);

    let children = state.data.children.lock().await;

    for child_partitionner in children.iter() {
        child_partitionner
            .remove_scene(payload.scene)
            .await
            .unwrap();
    }
}

async fn register_child(State(state): State<AppState>, Json(payload): Json<RegisterChildRequest>) {
    let mut children = state.data.children.lock().await;
    info!("Received child registration: {:?}", payload);
    let child_server =
        AsyncPartitionnerServer::with_endpoint(payload.child.addr, payload.child.port).unwrap();
    children.push(child_server);
}

async fn start_stop(State(state): State<AppState>, Json(payload): Json<StartStopRequest>) {
    let was_running = state.data.running.swap(payload.running, Ordering::SeqCst);

    if payload.running && !was_running {
        let scenes_ack = state.data.scenes_acks.read().await;
        if let Some(scene_ack) = scenes_ack.get(&payload.scene) {
            if scene_ack.step_id.load(Ordering::SeqCst)
                < scene_ack.step_limit.load(Ordering::SeqCst)
            {
                let step_id = scene_ack.step_id.load(Ordering::SeqCst);
                drop(scenes_ack);
                step(
                    State(state.clone()),
                    Json(StepRequest {
                        scene: payload.scene,
                        step_id,
                    }),
                )
                .await;
            } else {
                state.data.running.store(false, Ordering::SeqCst);
            }
        } else {
            drop(scenes_ack);
            step(
                State(state.clone()),
                Json(StepRequest {
                    scene: payload.scene,
                    step_id: 1,
                }),
            )
            .await;
        };
    }
}

async fn list_scenes(State(state): State<AppState>) -> Json<SceneList> {
    let runners = state.data.runners.lock().await;

    Json(SceneList {
        scenes: runners.per_node.keys().cloned().collect(),
    })
}

async fn list_regions(
    State(state): State<AppState>,
    Json(payload): Json<ListRegionsRequest>,
) -> Json<RegionList> {
    let runners = state.data.runners.lock().await;

    Json(RegionList {
        bounds: runners
            .assigned
            .iter()
            .filter(|((scene, _), _)| *scene == payload.scene)
            .map(|((_, region), _)| *region)
            .collect(),
    })
}

async fn insert_objects(
    State(state): State<AppState>,
    Json(payload): Json<InsertObjectsRequest>,
) -> Result<(), StatusCode> {
    log::info!("Inserting {} objects.", payload.bodies.len());

    if state
        .data
        .runners
        .lock()
        .await
        .exited
        .contains(&payload.scene)
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut locked_static_bodies = state.data.static_bodies.write().await;
    let static_bodies = locked_static_bodies
        .entry(payload.scene)
        .or_insert_with(|| vec![]);

    let mut region_to_objects = HashMap::new();

    // Group object by region.
    for body in payload.bodies {
        // TODO: not calculating islands here will induce a 1-step delay between the
        //       time the objects are simulated and the time they become aware of any
        //       potential island merge across the boundary.
        let aabb = body.cold.shape.compute_aabb(&body.warm.position);

        if body.cold.body_type.is_dynamic() {
            let region = SimulationBounds::from_aabb(&aabb, SimulationBounds::DEFAULT_WIDTH);
            region_to_objects
                .entry(region)
                .or_insert_with(Vec::new)
                .push(body);
        } else {
            static_bodies.push(body);
        }
    }

    // Send objects to runners.
    for (region, bodies) in region_to_objects {
        let runner = assign_runner(
            State(state.clone()),
            Json(AssignRunnerRequest {
                scene: payload.scene,
                region,
            }),
        )
        .await?;

        log::info!("Inserting {} objects to {}", bodies.len(), runner.uuid);

        // Send message to the runner.
        let message = RunnerMessage::AssignIsland {
            scene: payload.scene,
            region,
            bodies,
            impulse_joints: vec![],
        };
        put_runner_message(&state.data.zenoh, runner.uuid, message)
            .await
            .unwrap();
        let message = RunnerMessage::AssignStaticBodies {
            bodies: static_bodies.clone(),
        };
        put_runner_message(&state.data.zenoh, runner.uuid, message)
            .await
            .unwrap();
        put_runner_message(
            &state.data.zenoh,
            runner.uuid,
            RunnerMessage::SyncClientObjects,
        )
        .await
        .unwrap();
    }

    Ok(())
}

async fn runner_initialized(
    State(state): State<AppState>,
    Json(payload): Json<RunnerInitializedRequest>,
) {
    // let mut runners = state.data.runners.lock().await;
    //
    // if let Some(runner) = runners.uninitialized.remove(&payload.uuid) {
    //     log::info!("Runner {:?} acked initialization.", payload.uuid);
    //     runners.pending.push(runner);
    //     state.data.num_uninitialized.fetch_sub(1, Ordering::SeqCst);
    // }
}

async fn assign_runner(
    State(state): State<AppState>,
    Json(payload): Json<AssignRunnerRequest>,
) -> Result<Json<AssignRunnerResponse>, StatusCode> {
    // TODO: this basically makes this endpoint operate completely sequentially.
    //       How could we avoid this?
    let _lock_guard = state.data.assign_runner_lock.lock().await;

    let mut runners = state.data.runners.lock().await;

    if runners.exited.contains(&payload.scene) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if let Some(runner_uuid) = runners.assigned.get(&(payload.scene, payload.region)) {
        return Ok(Json(AssignRunnerResponse {
            scene: payload.scene,
            region: payload.region,
            uuid: *runner_uuid,
        }));
    }

    log::info!(
        "No runner assigned to region {:?}::{:?} yet.",
        payload.scene,
        payload.region,
    );

    // No runner exist for this region yet.
    match state.data.my_type {
        PartitionnerType::Master | PartitionnerType::Dev => {
            // This is a master partitionner, assign to one of its children.
            let children_bounds = state
                .data
                .scenes_geometries
                .read()
                .await
                .get(&payload.scene)
                .unwrap()
                .children_bounds
                .clone();
            let new_region_center = payload.region.aabb().center();

            let mut child_id = usize::MAX;

            for (id, child) in children_bounds.iter().enumerate() {
                if child.contains_local_point(&new_region_center) {
                    child_id = id;
                    break;
                }
            }

            if child_id == usize::MAX {
                // The new region is outside of the known bounds of the simulation, attach
                // it to the closest region.
                let mut closest = f32::MAX;
                for (id, child) in children_bounds.iter().enumerate() {
                    let child_cuboid = Cuboid::new(child.half_extents());
                    let child_pos = child.center().into();
                    let new_dist =
                        child_cuboid.distance_to_point(&child_pos, &new_region_center, true);
                    if new_dist < closest {
                        closest = new_dist;
                        child_id = id;
                    }
                }
            }

            let uuid = runners.per_node[&payload.scene][child_id].uuid;
            runners
                .assigned
                .insert((payload.scene, payload.region), uuid);

            log::info!(
                "Assigned region {:?}::{:?} to runner {:?}.",
                payload.scene,
                payload.region,
                uuid
            );

            Ok(Json(AssignRunnerResponse {
                scene: payload.scene,
                region: payload.region,
                uuid,
            }))
        }
        PartitionnerType::Runner => {
            unreachable!()
        }
    }
}

fn init_log() {
    let mut builder = env_logger::Builder::from_default_env();
    builder.filter_level(log::LevelFilter::Info);
    builder.init();
}

fn runner_stopped_child_wait_loop(to_remove: Receiver<Child>) {
    while let Ok(mut child) = to_remove.recv_blocking() {
        if let Err(e) = child.wait() {
            println!("Error waiting for runner to exit: {e}");
        }
    }
}

fn input_handling_loop(state: AppState) {
    tokio::spawn(async move {
        while let Ok(inputs) = state.data.inputs_rcv.recv().await {
            let scenes_acks = state.data.scenes_acks.read().await;
            if let Some(scene_acks) = scenes_acks.get(&inputs.scene) {
                let new_step_limit = inputs.step_id / NUM_INTERNAL_STEPS + 2;
                // println!(
                //     "################# Sim state: {} (old limit: {}, step id: {}) ################",
                //     new_step_limit,
                //     scene_acks.step_limit.load(Ordering::SeqCst),
                //     scene_acks.step_id.load(Ordering::SeqCst)
                // );

                scene_acks
                    .step_limit
                    .fetch_max(new_step_limit, Ordering::SeqCst);

                drop(scenes_acks);
                start_stop(
                    State(state.clone()),
                    Json(StartStopRequest {
                        scene: inputs.scene,
                        running: true,
                    }),
                )
                .await;
            }
        }
    });
}

async fn runner_init_validation_loop(state: AppState) {
    info!("Started runner init validation loop.");

    /*
    loop {
        // This condition is just to avoid some needless locking.
        if state.data.num_uninitialized.load(Ordering::SeqCst) > 0 {
            let locked_runners = state.data.runners.lock().await;
            info!(
                "Requesting validation from {} runners.",
                locked_runners.uninitialized.len()
            );
            for uninitialized_runner in locked_runners.uninitialized.values() {
                info!(
                    "Sending init validation request to runner {}",
                    uninitialized_runner.uuid
                );
                put_runner_message(
                    &state.data.zenoh,
                    uninitialized_runner.uuid,
                    RunnerMessage::RequestInitValidation,
                )
                .await
                .unwrap();
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
     */
}

pub async fn put_runner_message(
    zenoh: &ZenohContext,
    uuid: Uuid,
    message: RunnerMessage,
) -> anyhow::Result<()> {
    let message_str = serialize(&message)?;
    // FIXME: declare the publisher only once.
    //        The problem is that the publisher’s lifetime depends
    //        on both the zenoh session, and the zenoh key, lifetimes.
    let zenoh_key = runner_zenoh_commands_key(uuid);
    let publisher = zenoh
        .session
        .declare_publisher(&zenoh_key)
        .congestion_control(CongestionControl::Block)
        .res()
        .await
        .unwrap();
    publisher.put(message_str).res().await.unwrap();
    Ok(())
}
