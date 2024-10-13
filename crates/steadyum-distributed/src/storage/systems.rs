use crate::operation::{Operation, Operations};
use crate::render::{ColliderRender, ColliderRenderShape};
use crate::storage::db::{CameraPos, DbContext};
use crate::storage::position_interpolation::PositionInterpolation;
use crate::styling::ColorGenerator;
use crate::ui::UiState;
use crate::utils::{iso_to_transform, transform_to_iso, MissingDataPoints, PhysicsObject, Vect};
use crate::utils::{KinematicAnimationsComponent, RapierContext};
use crate::{block_on, MainCamera, PhysicsProgress};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::utils::{HashMap, Uuid};
use rapier::dynamics::{RigidBodyBuilder, RigidBodyType};
use rapier::math::Real;
use rapier::prelude::ColliderBuilder;
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use steadyum_api_types::messages::BodyAssignment;
use steadyum_api_types::objects::{ColdBodyObject, WarmBodyObject};
use steadyum_api_types::partitionner::SceneUuid;

pub fn update_start_stop(mut db: ResMut<DbContext>, ui: Res<UiState>) {
    let db = &mut *db;
    if db.is_running != ui.running {
        dbg!("Update start stop.");
        block_on(async {
            let scene = *db.scene.read().await;
            db.is_running = ui.running;
            db.partitionner
                .set_running(scene, db.is_running)
                .await
                .unwrap();
        });
    }
}

pub fn read_object_positions_from_kvs(
    mut commands: Commands,
    db: Res<DbContext>,
    mut progress: ResMut<PhysicsProgress>,
    mut colors: ResMut<ColorGenerator>,
    mut bodies: Query<(
        Entity,
        &Transform,
        &mut PhysicsObject,
        &mut MissingDataPoints,
        &mut PositionInterpolation,
        &mut ColliderRender,
        &mut Visibility,
    )>,
) {
    let mut new_progress_limit = u64::MAX;

    let mut min_progress_limit = u64::MAX;
    let mut max_progress_limit = 0;

    let t0 = instant::Instant::now();

    let got_new_region = db.read_new_region.swap(false, Ordering::SeqCst);
    let mut uuid_is_rendered = HashSet::new();

    let Some(uuid2body) = block_on(db.uuid2body.write()).take() else {
        return;
    };

    // println!("Found bodies: {}", uuid2body.len());

    for (entity, transform, mut object, mut missing, mut interpolation, mut color, mut visible) in
        bodies.iter_mut()
    {
        if let Some(data) = uuid2body.get(&object.uuid) {
            interpolation.add_interpolation_point(data.data.position, data.timestamp);
            object.sleeping = data.data.sleep_start_frame.is_some();

            let region_color = if data.data.body_type == RigidBodyType::Dynamic {
                colors.gen_region_color(data.bounds)
            } else {
                colors.static_object_color()
            };

            // NOTE: don’t trigger the color change detection if it didn’t change.
            if region_color != color.color {
                color.color = region_color;
            }

            min_progress_limit = min_progress_limit.min(interpolation.max_known_timestep());
            max_progress_limit = max_progress_limit.max(interpolation.max_known_timestep());

            missing.0 = 0;
            uuid_is_rendered.insert(object.uuid);
        } else {
            // HACK: while objects transition from region to region, they might be missing from the
            //       set read from the DB. So we allow a certain number of consecutive missing data
            //       before we actually remove the object.
            missing.0 += 1;

            if missing.0 > 5 {
                *visible = Visibility::Hidden;
                commands.entity(entity).despawn_recursive();
            }
        }
    }

    for (_, object) in uuid2body.into_iter() {
        if !uuid_is_rendered.contains(&object.data.uuid) {
            // This object has not been rendered yet.
            if got_new_region {
                progress.required_progress = progress.required_progress.max(object.timestamp);
            }

            let entity = commands.spawn((
                SpatialBundle::default(),
                PhysicsObject {
                    uuid: object.data.uuid,
                    sleeping: object.data.sleep_start_frame.is_some(),
                },
                PositionInterpolation::new(object.data.position, object.timestamp),
                ColliderRender::default(),
                MissingDataPoints(0),
                ColliderRenderShape {
                    shape: object.data.shape,
                },
            ));

            // if object.timestamp <
            // entity.insert(Visibility::Hidden);
        }
    }

    progress.calculated_progress_limits_range = [min_progress_limit, max_progress_limit];

    if min_progress_limit != u64::MAX {
        new_progress_limit = min_progress_limit; // (min_progress_limit + max_progress_limit) / 2;
    }

    // dbg!(latest_data.iter().count());
    if new_progress_limit != u64::MAX {
        progress.progress_limit = progress.progress_limit.max(new_progress_limit as usize);
    }

    if t0.elapsed().as_secs_f32() > 0.01 {
        // println!("read form kvs: {}", t0.elapsed().as_secs_f32());
    }
}

pub fn step_interpolations(
    ui_state: Res<UiState>,
    progress: Res<PhysicsProgress>,
    camera: Query<&GlobalTransform, With<MainCamera>>,
    mut objects: Query<(&mut PositionInterpolation, &mut Transform, &mut Visibility)>,
) {
    let t0 = instant::Instant::now();

    // println!(
    //     "Progress: {}/{}",
    //     progress.simulated_steps, progress.progress_limit
    // );

    let camera = camera.single();

    for (mut interpolation, mut transform, mut visibility) in objects.iter_mut() {
        interpolation.step(progress.simulated_steps as u64);

        let current_pos = if ui_state.interpolation {
            iso_to_transform(&interpolation.current_pos(), 1.0)
        } else {
            iso_to_transform(interpolation.final_pos(), 1.0)
        };

        // let new_visibility = if *body_ty == RigidBody::Dynamic
        //     && (current_pos.translation - camera.translation())
        //         .xz()
        //         .length()
        //         > 500.0
        // {
        //     Visibility::Hidden
        // } else {
        //     Visibility::Visible
        // };

        // if new_visibility != *visibility {
        //     *visibility = new_visibility;
        // }

        if *visibility != Visibility::Hidden {
            transform.translation = current_pos.translation;
            transform.rotation = current_pos.rotation;
        }
    }

    if t0.elapsed().as_secs_f32() > 0.01 {
        println!("step interpolations: {}", t0.elapsed().as_secs_f32());
    }
}

pub fn integrate_kinematic_animations(
    progress: Res<PhysicsProgress>,
    mut objects: Query<(&mut Transform, &KinematicAnimationsComponent)>,
) {
    for (mut transform, animations) in objects.iter_mut() {
        let base = transform_to_iso(&*transform, 1.0);
        let pos = animations.0.eval(progress.simulated_time, base);
        *transform = iso_to_transform(&pos, 1.0);
    }
}

pub fn update_camera_pos(db: Res<DbContext>, camera: Query<&Transform, With<MainCamera>>) {
    #[cfg(feature = "dim3")]
    for transform in camera.iter() {
        let camera_pos = CameraPos {
            position: transform.translation,
            dir: transform.rotation * -Vect::Z,
        };
        block_on(async { *db.camera.write().await = camera_pos });
    }
}

// TODO: move to its own file?
pub fn update_physics_progress(
    mut progress: ResMut<PhysicsProgress>,
    context: Res<RapierContext>,
    ui_state: Res<UiState>,
) {
    if ui_state.running {
        // println!(
        //     "sim steps: {}, limit: {}",
        //     progress.simulated_steps, progress.progress_limit
        // );
        if progress.simulated_steps <= progress.progress_limit {
            let mut progress_delta = 1;

            if progress.required_progress as usize > progress.simulated_steps {
                progress_delta = progress.required_progress as usize - progress.simulated_steps;
            }

            progress.simulated_time += context.integration_parameters.dt * progress_delta as Real;
            progress.simulated_steps += progress_delta;
        }
    } else {
        progress.simulated_steps = progress.progress_limit;
        progress.simulated_time =
            context.integration_parameters.dt * progress.simulated_steps as Real;
    }
}

pub fn handle_scene_reset(
    mut ui_state: ResMut<UiState>,
    mut progress: ResMut<PhysicsProgress>,
    db: ResMut<DbContext>,
    operations: Res<Operations>,
) {
    for op in operations.iter() {
        if let Operation::ClearScene = op {
            dbg!(">>>>>>>>>>>>>>>>>>>>>>>>> Clearing scene.");
            block_on(async {
                let scene = *db.scene.read().await;
                db.partitionner.remove_scene(scene).await.unwrap();
                let new_scene_uuid = Uuid::new_v4();
                db.scene.write().await.0 = new_scene_uuid;
                *db.uuid2body.write().await = None;
            });

            *progress = PhysicsProgress::default();
            ui_state.running = false;
        }
    }
}

pub fn open_existing_scene(
    mut ui_state: ResMut<UiState>,
    mut progress: ResMut<PhysicsProgress>,
    db: ResMut<DbContext>,
    operations: Res<Operations>,
) {
    for op in operations.iter() {
        if let Operation::LoadNetworkScene(uuid) = op {
            dbg!(">>>>>>>>>>>>>>>>>>>>>>>>> Clearing scene.");
            block_on(async {
                *db.scene.write().await = *uuid;
                *db.uuid2body.write().await = None;
            });

            *progress = PhysicsProgress::default();
            ui_state.running = false;
        }
    }
}

pub fn remove_scene_on_exit(mut exit: EventReader<AppExit>, db: ResMut<DbContext>) {
    for _ in exit.read() {
        dbg!("Bevy is exiting.");
        block_on(async {
            let scene = *db.scene.read().await;
            db.partitionner.remove_scene(scene).await.unwrap();
        });
    }
}

pub fn emit_client_inputs(
    db: Res<DbContext>,
    progress: Res<PhysicsProgress>,
    keyboard_input: Res<Input<KeyCode>>,
) {
    if db.is_running {
        block_on(async {
            let scene = *db.scene.read().await;

            let t0 = std::time::Instant::now();
            db.partitionner
                .client_input(scene, progress.simulated_steps as u64)
                .await
                .unwrap();

            if keyboard_input.just_released(KeyCode::Space) {
                let camera = db.camera.read().await.clone();
                let body = RigidBodyBuilder::dynamic()
                    .translation(camera.position.into())
                    .linvel((camera.dir * 100.0).into())
                    .build();
                let collider = ColliderBuilder::cuboid(
                    0.5 + rand::random::<f32>(),
                    0.5 + rand::random::<f32>(),
                    0.5 + rand::random::<f32>(),
                )
                .density(5.0)
                .friction(1.0)
                .build();

                let assignment = BodyAssignment {
                    uuid: Uuid::new_v4(),
                    warm: WarmBodyObject::from_body(&body, 0),
                    cold: ColdBodyObject::from_body_collider(&body, &collider),
                };
                db.partitionner
                    .insert_objects(scene, vec![assignment])
                    .await
                    .unwrap();
            }
            // println!(">>>>>>>>> TIME: {}", t0.elapsed().as_secs_f32());
        });
    }
}
