use crate::neighbors::NeighborRunner;
use crate::neighbors::Neighbors;
use crate::region_assignment::RegionAssignments;
use crate::runner::SimulationState;
use crate::AppState;
use futures::stream::FuturesUnordered;
use futures::{stream, StreamExt, TryStreamExt};
use rapier::parry::bounding_volume::{BoundingSphere, BoundingVolume};
use rapier::prelude::*;
use std::collections::HashMap;
use steadyum_api_types::objects::{ClientBodyObject, WarmBodyObject, WatchedObjects};
use steadyum_api_types::partitionner::SceneUuid;
use steadyum_api_types::region_db::AsyncPartitionnerServer;
use steadyum_api_types::serialization::deserialize;
use steadyum_api_types::simulation::SimulationBounds;
use steadyum_api_types::zenoh::ZenohContext;
use uuid::Uuid;
use zenoh::prelude::r#async::AsyncResolve;
use zenoh::prelude::SplitBuffer;
use zenoh::Session;

pub const WATCH_GROUP: Group = Group::GROUP_1;
pub const MAIN_GROUP: Group = Group::GROUP_2;

pub struct WatchedObject {
    pub region: SimulationBounds,
    pub watch_iteration_id: usize,
}

impl WatchedObject {
    pub fn new(region: SimulationBounds, watch_iteration_id: usize) -> Self {
        Self {
            region,
            watch_iteration_id,
        }
    }
}

pub type WatchedNeighbors = [WatchedNeighbor; 3];

pub enum WatchedNeighbor {
    Local {
        bounds: SimulationBounds,
    },
    Remote {
        uuid: Uuid,
        bounds: SimulationBounds,
    },
}

pub async fn init_watched_neighbors(
    app: &AppState,
    db: &AsyncPartitionnerServer,
    neighbors: &mut Neighbors<'_>,
    bounds: SimulationBounds,
) -> WatchedNeighbors {
    let watched_regions = [0, 1, 2].map(|i| {
        let mut shift = [0; 3];
        shift[i] = 1;
        bounds.relative_neighbor(shift)
    });
    neighbors
        .spawn_neighbors(app, db, app.scene, watched_regions.into_iter())
        .await;
    watched_regions.map(|bounds| match neighbors.fetch_neighbor(bounds) {
        NeighborRunner::Local { .. } => WatchedNeighbor::Local { bounds },
        NeighborRunner::Remote { uuid, .. } => {
            // log::info!("Found remote neighbor watch: {:?}/{:?}", uuid, bounds);
            WatchedNeighbor::Remote {
                uuid: *uuid,
                bounds,
            }
        }
    })
}

pub async fn read_watched_objects(
    app: &AppState,
    watched_neighbors: &WatchedNeighbors,
) -> Vec<(WatchedObjects, SimulationBounds)> {
    let mut result = vec![];
    let mut fetch_from_remote_futs = FuturesUnordered::new();

    let (snd, rcv) = async_channel::unbounded();

    for nbh in watched_neighbors {
        match nbh {
            WatchedNeighbor::Local { bounds } => {
                // log::info!("Querying local watch region: {:?}", bounds);
                let Some(watched) = app.watch_sets.get(bounds) else {
                    continue;
                };
                result.push((watched.clone(), *bounds));
            }
            WatchedNeighbor::Remote { uuid, bounds } => {
                let watch_key = bounds.watch_kvs_key(*uuid);
                let bounds = *bounds;

                let fetch_data_fut = async move {
                    // log::info!("Querying watch key: {}", watch_key);
                    let data = app.zenoh.session.get(watch_key).res_async().await;
                    (bounds, data)
                };
                fetch_from_remote_futs.push(fetch_data_fut);
            }
        }
    }

    {
        let snd = &snd;
        fetch_from_remote_futs
            .for_each_concurrent(None, |data| async {
                let _ = snd.send(data).await;
            })
            .await;
    }

    drop(snd);

    while let Ok((nbh, replies)) = rcv.recv().await {
        // log::info!("Found reply from {:?}.", nbh);
        let Ok(replies) = replies else { continue };
        let Ok(reply) = replies.recv() else { continue }; // NOTE: there should be only one reply.
        let Ok(sample) = reply.sample else { continue };
        let payload = sample.value.payload.contiguous();
        let data: WatchedObjects = deserialize(&payload).unwrap();
        // log::info!(
        //     "Reply from {:?} conatined {} objects.",
        //     nbh,
        //     data.objects.len()
        // );

        result.push((data, nbh));
    }

    result
}

pub fn compute_watch_data(
    sim_state: &SimulationState,
    num_steps_run: usize,
    reassignments: &RegionAssignments,
) -> WatchedObjects {
    let mut objects = vec![];
    let my_region_aabb = sim_state.sim_bounds.aabb();

    for (handle, body) in sim_state.bodies.iter() {
        if body.is_dynamic()
            && !sim_state.watched_objects.contains_key(&handle)
            && !reassignments.reassigned_bodies.contains(&handle)
        {
            let uuid = sim_state.body2uuid[&handle].clone();
            let predicted_pos = body.predict_position_using_velocity_and_forces(
                sim_state.params.dt * num_steps_run as f32,
            );

            let aabb = sim_state.colliders[body.colliders()[0]].compute_swept_aabb(&predicted_pos);

            // NOTE: object fully inside the region are not part of the watch set.
            if !my_region_aabb.contains(&aabb) {
                objects.push((uuid, aabb));
            }
        }
    }

    WatchedObjects { objects }
}
