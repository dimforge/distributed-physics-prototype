use crate::cli::CliArgs;
use crate::connected_components::calculate_connected_components;
use crate::neighbors::Neighbors;
use crate::region_assignment::{
    apply_and_send_region_assignments, calculate_region_assignments, RegionAssignments,
};
use crate::watch::{
    compute_watch_data, init_watched_neighbors, read_watched_objects, WatchedObject, MAIN_GROUP,
    WATCH_GROUP,
};
use crate::{AppState, RegionState};
use futures::TryFutureExt;
use log::info;
use rapier::data::Coarena;
use rapier::parry::partitioning::Qbvh;
use rapier::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use steadyum_api_types::kinematic::KinematicAnimations;
use steadyum_api_types::messages::{BodyAssignment, RunnerMessage};
use steadyum_api_types::objects::{
    ClientBodyObject, ClientBodyObjectSet, ColdBodyObject, WarmBodyObject, WatchedObjects,
};
use steadyum_api_types::partitionner::{SceneUuid, NUM_INTERNAL_STEPS};
use steadyum_api_types::region_db::AsyncPartitionnerServer;
use steadyum_api_types::serialization::{deserialize, serialize};
use steadyum_api_types::simulation::SimulationBounds;
use steadyum_api_types::zenoh::{runner_zenoh_commands_key, ZenohContext};
use uuid::Uuid;
use zenoh::config::WhatAmI;
use zenoh::prelude::r#async::AsyncResolve;
use zenoh::prelude::SplitBuffer;
use zenoh::subscriber::Reliability;

pub struct QueryableWatchedObjects {
    pub qbvh: Qbvh<usize>,
    pub objects: Vec<(SimulationBounds, Aabb)>,
}

#[derive(Default, Clone, Copy)]
pub struct BodyAttributes {
    pub sleep_step_id: Option<u64>,
}

#[derive(Default)]
pub struct SimulationState {
    pub scene: SceneUuid,
    pub step_id: u64,
    pub killed: bool,
    pub query_pipeline: QueryPipeline,
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    pub gravity: Vector<f32>,
    pub params: IntegrationParameters,
    pub islands: IslandManager,
    pub broad_phase: BroadPhase,
    pub narrow_phase: NarrowPhase,
    pub impulse_joints: ImpulseJointSet,
    pub multibody_joints: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub physics_pipeline: PhysicsPipeline,
    pub body2animations: Coarena<KinematicAnimations>,
    pub body2uuid: HashMap<RigidBodyHandle, Uuid>,
    pub uuid2body: HashMap<Uuid, RigidBodyHandle>,
    pub sim_bounds: SimulationBounds,
    pub watched_objects: HashMap<RigidBodyHandle, WatchedObject>,
    pub bodies_attributes: Coarena<BodyAttributes>,
}

#[derive(Copy, Clone, Debug, Default)]
struct MainLoopTimings {
    pub num_bodies: usize,
    pub waiting_acks: f32,
    pub read_watch_sets: f32,
    pub apply_watch_sets: f32,
    pub resolve_assignments: f32,
    pub message_processing: f32,
    pub simulation_step: f32,
    pub connected_components: f32,
    pub data_and_watch_list: f32,
    pub release_reassign: f32,
    pub ack: f32,
    pub loop_time: f32,
}

pub async fn run_simulation(reg_state: RegionState) -> anyhow::Result<()> {
    let my_uuid = reg_state.uuid;
    let mut neighbors = Neighbors::new(&reg_state.app.zenoh);
    let mut sim_state = SimulationState::default();
    sim_state.sim_bounds = reg_state.bounds;
    sim_state.scene = reg_state.app.scene;
    sim_state.gravity = Vector::y() * (-9.81);

    // Subscribe to command queue.
    let mut watch_iteration_id = 0;

    /*
     * Wait for region assignment (blocking).
     */
    let mut pending_assignments = vec![];
    let mut static_bodies_added = 0;

    /*
     * Main runner loop.
     */
    let watched_neighbors = init_watched_neighbors(
        &reg_state.app,
        &reg_state.app.main_partitionner,
        &mut neighbors,
        sim_state.sim_bounds,
    )
    .await;

    'stop: while !sim_state.killed {
        let mut timings = MainLoopTimings::default();
        let loop_time = std::time::Instant::now();

        let t0 = std::time::Instant::now();

        // Process messages.
        while let Ok(message) = reg_state.reg_rcv.recv().await {
            if let RunnerMessage::Step { step_id } = &message {
                sim_state.step_id = *step_id;
                break;
            }

            process_message(
                &reg_state.app,
                my_uuid,
                &mut sim_state,
                message,
                &mut pending_assignments,
            )
            .await?;

            if sim_state.killed {
                break 'stop;
            }
        }

        // Add any missing static body.
        {
            let static_bodies_in_scene = reg_state.app.static_bodies.read().await;
            if static_bodies_in_scene.len() > static_bodies_added {
                // info!(
                //     "Adding static bodies to simulation: {}",
                //     static_bodies_in_scene.len() - static_bodies_added
                // );
                pending_assignments
                    .extend_from_slice(&static_bodies_in_scene[static_bodies_added..]);
                static_bodies_added = static_bodies_in_scene.len();
            }
        }

        // info!(
        //     "[{}] neighbor timestamp: {:?}, my step id: {}",
        //     my_uuid, timestamp, sim_state.step_id
        // );

        timings.waiting_acks = t0.elapsed().as_secs_f32();

        let t0 = std::time::Instant::now();
        watch_iteration_id += 1;
        let watched: Vec<(WatchedObjects, SimulationBounds)> =
            read_watched_objects(&reg_state.app, &watched_neighbors).await;
        timings.read_watch_sets = t0.elapsed().as_secs_f32();

        let t0 = std::time::Instant::now();
        let mut watched_objects_tree = Qbvh::new();

        let all_watched_objects: Vec<_> = watched
            .iter()
            .flat_map(|(objs, region)| objs.objects.iter().map(|o| (*region, o.1)))
            .collect();
        watched_objects_tree.clear_and_rebuild(
            all_watched_objects
                .iter()
                .enumerate()
                .map(|(i, (_, aabb))| (i, *aabb)),
            0.0,
        );

        let queryable_watched_objects = QueryableWatchedObjects {
            qbvh: watched_objects_tree,
            objects: all_watched_objects,
        };
        timings.apply_watch_sets = t0.elapsed().as_secs_f32();

        let t0 = std::time::Instant::now();
        resolve_pending_assignments(&mut sim_state, &mut pending_assignments);
        timings.resolve_assignments = t0.elapsed().as_secs_f32();

        let mut region_assignments = RegionAssignments::default();

        let t0 = std::time::Instant::now();

        for sub_step_id in 0..NUM_INTERNAL_STEPS {
            sim_state.physics_pipeline.step(
                &sim_state.gravity,
                &sim_state.params,
                &mut sim_state.islands,
                &mut sim_state.broad_phase,
                &mut sim_state.narrow_phase,
                &mut sim_state.bodies,
                &mut sim_state.colliders,
                &mut sim_state.impulse_joints,
                &mut sim_state.multibody_joints,
                &mut sim_state.ccd_solver,
                None,
                &(),
                &(),
            );

            let current_physics_time = (reg_state.step_id() * NUM_INTERNAL_STEPS + sub_step_id + 1)
                as Real
                * sim_state.params.dt;

            // Update animations.
            for (handle, animations) in sim_state.body2animations.iter() {
                if animations.linear.is_none() && animations.angular.is_none() {
                    // Nothing to animate.
                    continue;
                }

                // println!("Animating: {:?}.", handle);
                if let Some(rb) = sim_state.bodies.get_mut(RigidBodyHandle(handle)) {
                    let new_pos = animations.eval(current_physics_time, *rb.position());
                    // TODO: what if it’s a velocity-based kinematic body?
                    // println!("prev: {:?}, new: {:?}", rb.position(), new_pos);
                    rb.set_next_kinematic_position(new_pos);
                }
            }
        }

        timings.simulation_step = t0.elapsed().as_secs_f32();

        let num_steps_run = NUM_INTERNAL_STEPS;

        let t0 = std::time::Instant::now();

        let connected_components =
            calculate_connected_components(&sim_state, num_steps_run as usize);
        region_assignments = calculate_region_assignments(
            &mut sim_state,
            connected_components,
            &queryable_watched_objects,
        );
        timings.connected_components = t0.elapsed().as_secs_f32();

        let t0 = std::time::Instant::now();

        let client_objects = compute_client_objects(&mut sim_state, &[]);
        let watched = compute_watch_data(&sim_state, num_steps_run as usize, &region_assignments);

        timings.data_and_watch_list = t0.elapsed().as_secs_f32();

        let t0 = std::time::Instant::now();

        /*
         * Upload the new positions for clients, as well as the watch set.
         */
        {
            // println!(
            //     ">>>>> Inserted in watch set: {:?}, num: {}",
            //     sim_state.sim_bounds,
            //     watched.objects.len()
            // );
            reg_state
                .app
                .watch_sets
                .insert(sim_state.sim_bounds, watched);
            reg_state
                .app
                .client_object_sets
                .insert(sim_state.sim_bounds, client_objects);

            /*
             * Send objects to adjacent regions if assignment changed.
             */
            apply_and_send_region_assignments(
                &reg_state.app,
                &mut sim_state,
                &region_assignments,
                &mut neighbors,
                &reg_state.app.main_partitionner,
            )
            .await
            .unwrap();
        }

        timings.release_reassign = t0.elapsed().as_secs_f32();

        let t0 = std::time::Instant::now();

        // Send the ack. Note that this must not be run concurrently with the previous
        // future since we need to ack only after all the data was uploaded.
        reg_state
            .app
            .main_thread_snd
            .send(RunnerMessage::Ack)
            .await?;
        timings.ack = t0.elapsed().as_secs_f32();

        timings.loop_time = loop_time.elapsed().as_secs_f32();
        timings.num_bodies = sim_state.bodies.len();

        // info!("Runner {my_uuid} timings: {:?}", timings);
    }

    info!("Runner {my_uuid} is exiting. Bye bye!");

    Ok(())
}

fn make_builders(
    cold_object: &ColdBodyObject,
    warm_object: WarmBodyObject,
) -> (RigidBodyBuilder, ColliderBuilder) {
    let body = RigidBodyBuilder::new(cold_object.body_type)
        .position(warm_object.position)
        .linvel(warm_object.linvel)
        .angvel(warm_object.angvel);
    let collider = ColliderBuilder::new(cold_object.shape.clone()).density(cold_object.density);
    (body, collider)
}

fn resolve_pending_assignments(
    sim_state: &mut SimulationState,
    pending_assignments: &mut Vec<BodyAssignment>,
) {
    pending_assignments.retain(|data| {
        if data.warm.timestamp > sim_state.step_id {
            println!("{} > {}", data.warm.timestamp, sim_state.step_id);
            // This body lives in the future, we can’t simulate it for now.
            return true;
        }

        if let Some(handle) = sim_state.uuid2body.get(&data.uuid) {
            sim_state.bodies.remove(
                *handle,
                &mut sim_state.islands,
                &mut sim_state.colliders,
                &mut sim_state.impulse_joints,
                &mut sim_state.multibody_joints,
                true,
            );
            sim_state.watched_objects.remove(handle);
        }

        let (body, collider) = make_builders(&data.cold, data.warm);
        let watch_shape_radius = collider.shape.compute_local_bounding_sphere().radius * 1.1;
        let body_handle = sim_state.bodies.insert(body);
        sim_state
            .colliders
            .insert_with_parent(collider, body_handle, &mut sim_state.bodies);
        let watch_collider = ColliderBuilder::ball(watch_shape_radius)
            .density(0.0)
            .collision_groups(InteractionGroups::new(
                // We don’t care about watched objects intersecting each others.
                WATCH_GROUP,
                MAIN_GROUP,
            ))
            // Watched objects don’t generate forces.
            .solver_groups(InteractionGroups::none());
        sim_state
            .colliders
            .insert_with_parent(watch_collider, body_handle, &mut sim_state.bodies);
        sim_state.body2uuid.insert(body_handle, data.uuid.clone());
        sim_state.uuid2body.insert(data.uuid, body_handle);
        sim_state
            .body2animations
            .insert(body_handle.0, data.cold.animations.clone());

        // for data in impulse_joints {
        //     if let (Some(handle1), Some(handle2)) = (
        //         sim_state.uuid2body.get(&data.body1),
        //         sim_state.uuid2body.get(&data.body2),
        //     ) {
        //         sim_state
        //             .impulse_joints
        //             .insert(*handle1, *handle2, data.joint, true);
        //     }
        // }

        false
    });
}

async fn process_message(
    app: &AppState,
    my_uuid: Uuid,
    sim_state: &mut SimulationState,
    message: RunnerMessage,
    pending_assignments: &mut Vec<BodyAssignment>,
) -> anyhow::Result<()> {
    match message {
        RunnerMessage::Exit => {
            sim_state.killed = true;
        }
        RunnerMessage::AssignIsland {
            mut bodies,
            impulse_joints,
            ..
        } => {
            // info!(
            //     "[{}] adding {} bodies and {} imp. joints",
            //     my_uuid,
            //     bodies.len(),
            //     impulse_joints.len()
            // );
            pending_assignments.append(&mut bodies);
        }
        RunnerMessage::SyncClientObjects => {
            let client_objects = compute_client_objects(sim_state, &pending_assignments);
            app.client_object_sets
                .insert(sim_state.sim_bounds, client_objects);
        }
        RunnerMessage::AssignStaticBodies { .. }
        | RunnerMessage::Ack
        | RunnerMessage::Step { .. } => unreachable!(),
    }

    Ok(())
}

fn compute_client_objects(
    sim_state: &mut SimulationState,
    pending: &[BodyAssignment],
) -> ClientBodyObjectSet {
    let timestamp = sim_state.step_id * NUM_INTERNAL_STEPS;
    let mut objects = vec![];

    for (handle, body) in sim_state.bodies.iter() {
        if !sim_state.watched_objects.contains_key(&handle) {
            let warm_object = WarmBodyObject::from_body(body, timestamp);
            let uuid = sim_state.body2uuid[&handle].clone();

            sim_state
                .bodies_attributes
                .ensure_element_exist(handle.0, BodyAttributes::default());

            let attrs = sim_state.bodies_attributes.get_mut(handle.0).unwrap();
            if body.is_sleeping() {
                if attrs.sleep_step_id.is_none() {
                    attrs.sleep_step_id = Some(timestamp);
                }
            } else {
                attrs.sleep_step_id = None;
            }

            let client_object = ClientBodyObject {
                uuid,
                position: warm_object.position,
                shape: sim_state.colliders[body.colliders()[0]]
                    .shared_shape()
                    .clone(),
                body_type: body.body_type(),
                sleep_start_frame: attrs.sleep_step_id,
            };
            objects.push(client_object);
        }
    }

    for pending in pending {
        let client_object = ClientBodyObject {
            uuid: pending.uuid,
            position: pending.warm.position,
            shape: pending.cold.shape.clone(),
            body_type: pending.cold.body_type,
            sleep_start_frame: None,
        };
        objects.push(client_object);
    }

    ClientBodyObjectSet { timestamp, objects }
}
