use crate::cli::CliArgs;
use crate::rapier::dynamics::RigidBodyHandle;
use crate::utils::Vect;
use bevy::prelude::Resource;
use bevy::utils::Uuid;
use dashmap::DashMap;
use futures::{stream, StreamExt};
use rapier::geometry::HalfSpace;
use rapier::math::Vector;
use rapier::parry::bounding_volume::{Aabb, BoundingVolume};
use sled::Atomic;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;
use steadyum_api_types::env::CONFIG;
use steadyum_api_types::messages::{BodyAssignment, ImpulseJointAssignment};
use steadyum_api_types::objects::{
    ClientBodyObject, ClientBodyObjectSet, ColdBodyObject, RegionList, WarmBodyObject,
};
use steadyum_api_types::partitionner::SceneUuid;
use steadyum_api_types::region_db::AsyncPartitionnerServer;
use steadyum_api_types::serialization::deserialize;
use steadyum_api_types::simulation::SimulationBounds;
use steadyum_api_types::zenoh::ZenohContext;
use tokio::sync::RwLock;
use zenoh::config::WhatAmI;
use zenoh::prelude::r#async::AsyncResolve;
use zenoh::prelude::SplitBuffer;

pub struct NewObjectCommand {
    pub uuid: Uuid,
    // TODO: keep this?
    pub handle: RigidBodyHandle,
    pub cold_object: ColdBodyObject,
    pub warm_object: WarmBodyObject,
}

pub enum DbCommand {
    NewScene { objects: Vec<NewObjectCommand> },
}

#[derive(Clone)]
pub struct LatestBodyData {
    pub bounds: SimulationBounds,
    pub timestamp: u64,
    pub data: ClientBodyObject,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct CameraPos {
    pub position: Vect,
    pub dir: Vect,
}

impl CameraPos {
    pub fn visible_regions(&self) -> Vec<SimulationBounds> {
        let camera_a = self.position;
        let camera_b = self.position + self.dir * SimulationBounds::DEFAULT_WIDTH as f32 * 20.0;
        let camera_aabb = Aabb::new(camera_a.min(camera_b).into(), camera_a.max(camera_b).into());

        SimulationBounds::intersecting_aabb(camera_aabb, SimulationBounds::DEFAULT_WIDTH)
    }
}

#[derive(Default, Debug)]
pub struct DbStats {
    pub total_num_regions: AtomicUsize,
    pub num_visible_regions: AtomicUsize,
    pub num_objects_read: AtomicUsize,
    pub total_db_read_time_ms: AtomicUsize,
}

#[derive(Resource)]
pub struct DbContext {
    pub commands_snd: async_channel::Sender<DbCommand>,
    pub camera: Arc<RwLock<CameraPos>>,
    pub uuid2body: Arc<RwLock<Option<HashMap<Uuid, LatestBodyData>>>>,
    pub region_list: Arc<RwLock<RegionList>>,
    pub partitionner: Arc<AsyncPartitionnerServer>,
    pub scene: Arc<RwLock<SceneUuid>>,
    pub read_new_region: Arc<AtomicBool>,
    pub stats: Arc<DbStats>,
    pub is_running: bool,
    pub runtime: tokio::runtime::Runtime,
}

pub fn spawn_db_thread(local_dev_mode: bool) -> DbContext {
    let (commands_snd, commands_rcv) = async_channel::unbounded();
    let camera = Arc::new(RwLock::new(CameraPos::default()));
    let uuid2body = Arc::new(RwLock::new(None));
    let region_list = Arc::new(RwLock::new(RegionList::default()));
    let partitionner = Arc::new(AsyncPartitionnerServer::new().unwrap());
    let scene = Arc::new(RwLock::new(SceneUuid(Uuid::new_v4())));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let read_new_region = Arc::new(AtomicBool::new(true));
    let stats = Arc::new(DbStats::default());

    {
        let partitionner = partitionner.clone();
        let scene = scene.clone();

        runtime.spawn(async move {
            /*
             * Command loop.
             */
            while let Ok(command) = commands_rcv.recv().await {
                match command {
                    DbCommand::NewScene { objects } => {
                        let scene_uuid = *scene.read().await;
                        let mut scene_aabb = Aabb::new_invalid();
                        for obj in &objects {
                            // Don’t count halfspaces, they are infinite.
                            if obj.cold_object.shape.is::<HalfSpace>() {
                                continue;
                            }

                            let obj_aabb = obj
                                .cold_object
                                .shape
                                .compute_aabb(&obj.warm_object.position);
                            scene_aabb.merge(&obj_aabb);
                        }

                        partitionner
                            .create_scene(scene_uuid, scene_aabb)
                            .await
                            .unwrap();
                        for objects in objects.chunks(1024) {
                            let bodies_to_insert: Vec<_> = objects
                                .iter()
                                .map(|obj| BodyAssignment {
                                    uuid: obj.uuid,
                                    cold: obj.cold_object.clone(),
                                    warm: obj.warm_object.clone(),
                                })
                                .collect();
                            dbg!("Sending objects query to the partitionner!");
                            partitionner
                                .insert_objects(scene_uuid, bodies_to_insert)
                                .await
                                .unwrap();
                        }
                    }
                }
            }

            Ok::<(), anyhow::Error>(())
        });
    }

    {
        let region_list = region_list.clone();
        let partitionner = partitionner.clone();
        let scene = scene.clone();
        let uuid2body = uuid2body.clone();
        let mut fetched_uuid2body = HashMap::new();
        let camera = camera.clone();
        let read_new_region = read_new_region.clone();
        let stats = stats.clone();

        // let camera = camera.clone();
        runtime.spawn(async move {
            let mut prev_region_list = HashSet::new();
            let mut known_region_timestamps = HashMap::new();

            /*
             * Init S3
             */
            let whatami = if local_dev_mode {
                WhatAmI::Peer
            } else {
                WhatAmI::Client
            };
            let mut zenoh = ZenohContext::new(whatami, Some(CONFIG.zenoh_router.clone()), false)
                .await
                .unwrap();

            /*
             * Position reading loop.
             */
            loop {
                let t0 = std::time::Instant::now();

                let scene = *scene.read().await;

                // TODO: we should be able to query the scene with an AABB or something.
                let mut new_region_list: RegionList =
                    partitionner.list_regions(scene).await.unwrap_or_default();

                stats.total_num_regions.store(
                    new_region_list.bounds.len(),
                    std::sync::atomic::Ordering::SeqCst,
                );

                let camera_pos = camera.read().await.clone();
                let view_aabb =
                    Aabb::from_half_extents(camera_pos.position.into(), Vector::repeat(750.0));
                new_region_list
                    .bounds
                    .retain(|bounds| bounds.intersects_aabb(&view_aabb));

                stats.num_visible_regions.store(
                    new_region_list.bounds.len(),
                    std::sync::atomic::Ordering::SeqCst,
                );

                let replies: Vec<_> = stream::iter(new_region_list.bounds.iter())
                    .then(|bounds| async {
                        let storage_key = bounds.runner_client_objects_key(
                            scene,
                            known_region_timestamps.get(bounds).copied().unwrap_or(0),
                        );
                        zenoh.session.get(&storage_key).res_async().await
                    })
                    .collect()
                    .await;

                let mut num_objects_read = 0;
                for (reply, bounds) in replies.into_iter().zip(new_region_list.bounds.iter()) {
                    let Ok(reply) = reply else { continue };

                    while let Ok(reply) = reply.recv() {
                        let Ok(sample) = reply.sample else { continue };
                        let payload = sample.value.payload.contiguous();
                        let data: ClientBodyObjectSet = deserialize(&payload).unwrap();

                        known_region_timestamps.insert(*bounds, data.timestamp);
                        num_objects_read += data.objects.len();

                        for object in data.objects {
                            let uuid = object.uuid;
                            let data = LatestBodyData {
                                bounds: *bounds,
                                timestamp: data.timestamp,
                                data: object,
                            };

                            fetched_uuid2body.insert(uuid, data);
                        }
                    }
                }

                fetched_uuid2body.retain(|_, body| {
                    if let Some(actual_timestamp) = known_region_timestamps.get(&body.bounds) {
                        if body.data.sleep_start_frame.is_some() {
                            /* Keep the body, just update the timestamp, it is sleeping so we don’t get new updates. */
                            body.timestamp = *actual_timestamp;
                        }

                        body.timestamp >= *actual_timestamp
                    } else {
                        false
                    }
                });

                stats
                    .num_objects_read
                    .store(num_objects_read, std::sync::atomic::Ordering::SeqCst);

                for reg in &new_region_list.bounds {
                    if !prev_region_list.contains(reg) {
                        read_new_region.store(true, std::sync::atomic::Ordering::SeqCst);
                        break;
                    }
                }

                prev_region_list.clear();
                prev_region_list.extend(new_region_list.bounds.iter().copied());

                // println!("Apply async time: {}", t0.elapsed().as_secs_f32());
                known_region_timestamps.retain(|region, _| {
                    region.intersects_aabb(&view_aabb)
                });
                fetched_uuid2body.retain(|_, body| {
                    known_region_timestamps.contains_key(&body.bounds)
                });

                *uuid2body.write().await = Some(fetched_uuid2body.clone());
                *region_list.write().await = new_region_list;

                stats.total_db_read_time_ms.store(
                    t0.elapsed().as_millis() as usize,
                    std::sync::atomic::Ordering::SeqCst,
                );
            }
        });
    }

    DbContext {
        commands_snd,
        scene,
        uuid2body,
        camera,
        region_list,
        read_new_region,
        partitionner,
        is_running: false,
        runtime,
        stats,
    }
}
