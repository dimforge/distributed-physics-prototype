use crate::connected_components::ConnectedComponent;
use crate::neighbors::Neighbors;
use crate::runner::{QueryableWatchedObjects, SimulationState};
use crate::watch::WatchedObject;
use crate::AppState;
use futures::{stream, StreamExt, TryStreamExt};
use rapier::parry::bounding_volume::BoundingVolume;
use rapier::parry::partitioning::Qbvh;
use rapier::prelude::*;
use std::collections::{HashMap, HashSet};
use steadyum_api_types::messages::{BodyAssignment, RunnerMessage};
use steadyum_api_types::objects::{ColdBodyObject, WarmBodyObject};
use steadyum_api_types::region_db::AsyncPartitionnerServer;
use steadyum_api_types::simulation::SimulationBounds;

const MIN_SENDBACK_DELAY: u128 = 50;

#[derive(Default)]
pub struct RegionAssignments {
    /// Region assignment based on connected components.
    pub bodies_to_reassign: HashMap<SimulationBounds, Vec<RigidBodyHandle>>,
    /// Set of rigid-body that should change region.
    pub reassigned_bodies: HashSet<RigidBodyHandle>, // TODO: coarena?
}

pub fn calculate_region_assignments(
    sim_state: &mut SimulationState,
    connected_components: Vec<ConnectedComponent>,
    watched_objects: &QueryableWatchedObjects,
) -> RegionAssignments {
    let mut result = RegionAssignments::default();
    let mut watch_intersections = vec![];

    // FIXME: this is an extremely conservative method to grab all
    //        the rigid-bodies that are likely to re-enter the current
    //        region due to interference with watch set. The proper,
    //        solution is to compute AABBs of individual
    //        islands that would have ended up outside of the current
    //        region, and check if the swept aabb of that connected component
    //        intersects any of the objects that we intend to add to the watch set.
    let mut watch_aabb = Aabb::new_invalid();
    let mut cc_regions = vec![sim_state.sim_bounds; connected_components.len()];

    for (cc_id, connected_component) in connected_components.iter().enumerate() {
        if connected_component.bodies.is_empty() {
            continue;
        }

        let mut best_region = SimulationBounds::smallest();

        for handle in &connected_component.bodies {
            let candidate_region = sim_state
                .watched_objects
                .get(handle)
                .map(|watched| watched.region)
                .unwrap_or_else(|| {
                    let body = &sim_state.bodies[*handle];
                    let aabb = sim_state.colliders[body.colliders()[0]].compute_aabb();

                    watched_objects
                        .qbvh
                        .intersect_aabb(&aabb, &mut watch_intersections);

                    if watch_intersections.is_empty() {
                        let body_region =
                            SimulationBounds::from_aabb(&aabb, SimulationBounds::DEFAULT_WIDTH);

                        if body_region < sim_state.sim_bounds {
                            if body.user_data < MIN_SENDBACK_DELAY {
                                sim_state.bodies[*handle].user_data += 1;
                                return sim_state.sim_bounds;
                            }
                        }

                        body_region
                    } else {
                        watch_intersections
                            .drain(..)
                            .map(|i| watched_objects.objects[i].0)
                            .max()
                            .unwrap()
                    }
                });

            if candidate_region > best_region {
                best_region = candidate_region;
            }
        }

        cc_regions[cc_id] = best_region;

        if best_region == sim_state.sim_bounds {
            watch_aabb.merge(&connected_component.swept_aabb);
        }
    }

    let mut num_reassigned = 0;
    let mut num_smaller = 0;
    let mut num_bigger = 0;
    for (cc_region, cc) in cc_regions.iter().zip(connected_components.iter()) {
        if *cc_region < sim_state.sim_bounds {
            num_smaller += 1;
        } else if *cc_region > sim_state.sim_bounds {
            num_bigger += 1;
        }

        if *cc_region > sim_state.sim_bounds
            || (*cc_region < sim_state.sim_bounds/* && !watch_aabb.intersects(&cc.swept_aabb) */)
        {
            num_reassigned += 1;
            let region = result
                .bodies_to_reassign
                .entry(*cc_region)
                .or_insert_with(Vec::new);
            region.extend(
                cc.bodies
                    .iter()
                    .filter(|h| !sim_state.watched_objects.contains_key(&h))
                    .copied(),
            );
            result.reassigned_bodies.extend(
                cc.bodies
                    .iter()
                    .filter(|h| !sim_state.watched_objects.contains_key(&h))
                    .copied(),
            );
        }
    }

    println!(
        ">>>>>>>>>>>>>>> Num CC: {}, num smaller: {}, num bigger: {}, reassigned: {}",
        connected_components.len(),
        num_smaller,
        num_bigger,
        num_reassigned
    );

    result
}

pub async fn apply_and_send_region_assignments(
    app_state: &AppState,
    sim_state: &mut SimulationState,
    assignments: &RegionAssignments,
    neighbors: &mut Neighbors<'_>,
    db_context: &AsyncPartitionnerServer,
) -> anyhow::Result<()> {
    neighbors
        .spawn_neighbors(
            app_state,
            db_context,
            sim_state.scene,
            assignments.bodies_to_reassign.iter().map(|(r, _)| *r),
        )
        .await;

    {
        let bodies_to_reassign: futures::stream::FuturesUnordered<_> = assignments
            .bodies_to_reassign
            .iter()
            .map(futures::future::ok)
            .collect();
        bodies_to_reassign
            .try_for_each_concurrent(None, |(new_region, handles)| async {
                if handles.is_empty() {
                    return Ok::<_, anyhow::Error>(());
                }

                let neighbor = neighbors.fetch_neighbor(*new_region);

                let body_assignments = handles
                    .iter()
                    .map(|handle| {
                        let body = &sim_state.bodies[*handle];
                        let collider = &sim_state.colliders[body.colliders()[0]];
                        let uuid = sim_state.body2uuid[handle];
                        let warm = WarmBodyObject::from_body(body, sim_state.step_id);
                        let cold = ColdBodyObject::from_body_collider(body, collider);
                        BodyAssignment { uuid, warm, cold }
                    })
                    .collect();

                // Switch region.
                let message = RunnerMessage::AssignIsland {
                    scene: app_state.scene,
                    region: *new_region,
                    bodies: body_assignments,
                    impulse_joints: vec![],
                };

                neighbor.send(&message).await?;
                Ok(())
            })
            .await?;
    }

    for handle in assignments
        .bodies_to_reassign
        .values()
        .flat_map(|handles| handles.iter())
    {
        sim_state.bodies.remove(
            *handle,
            &mut sim_state.islands,
            &mut sim_state.colliders,
            &mut sim_state.impulse_joints,
            &mut sim_state.multibody_joints,
            true,
        );
        if let Some(uuid) = sim_state.body2uuid.remove(handle) {
            sim_state.uuid2body.remove(&uuid);
        }
    }

    Ok(())
}
