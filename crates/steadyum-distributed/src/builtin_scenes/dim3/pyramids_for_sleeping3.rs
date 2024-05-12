use crate::builtin_scenes::BuiltinScene;
use crate::utils::RapierContext;
use na::Vector3;
use rapier::prelude::*;
use std::collections::HashMap;
use steadyum_api_types::kinematic::{KinematicAnimations, KinematicCurve};

fn create_wall(
    bodies: &mut RigidBodySet,
    colliders: &mut ColliderSet,
    offset: Vector<f32>,
    stack_height: usize,
    half_extents: Vector<f32>,
) {
    let shift = half_extents * 2.0;
    for i in 0usize..stack_height {
        for j in i..stack_height {
            let fj = j as f32;
            let fi = i as f32;
            let x = offset.x;
            let y = fi * shift.y + offset.y;
            let z = (fi * shift.z / 2.0) + (fj - fi) * shift.z + offset.z
                - stack_height as f32 * half_extents.z;

            // Build the rigid body.
            let rigid_body = RigidBodyBuilder::dynamic().translation(vector![x, y, z]);
            let handle = bodies.insert(rigid_body);
            let collider = ColliderBuilder::cuboid(half_extents.x, half_extents.y, half_extents.z);
            // let collider = ColliderBuilder::ball(half_extents.y);
            colliders.insert_with_parent(collider, handle, bodies);
        }
    }
}

const GROUND_SIZE: f32 = 350.0;

fn init_platform_with_walls(
    result: &mut RapierContext,
    animations: &mut HashMap<RigidBodyHandle, KinematicAnimations>,
    platform_shift: Vector3<f32>,
) {
    /*
     * Ground
     */
    let ground_height = 5.0;

    let rigid_body = RigidBodyBuilder::kinematic_position_based()
        .translation(platform_shift + Vector3::y() * (-ground_height + 25.0));
    let ground_handle = result.bodies.insert(rigid_body);
    let n = 10;

    let collider = ColliderBuilder::cuboid(GROUND_SIZE, ground_height, GROUND_SIZE);
    result
        .colliders
        .insert_with_parent(collider, ground_handle, &mut result.bodies);

    let ground_animation = KinematicAnimations {
        linear: None,
        angular: Some(KinematicCurve {
            control_points: vec![vector![0.0, 0.0, 0.0], vector![0.0, 1.0, 0.0]],
            t0: 0.0,
            total_time: 10.0,
            loop_back: false,
        }),
    };
    animations.insert(ground_handle, ground_animation);

    /*
     * Create the pyramids.
     */
    let num_basis = 7;
    let num_z = 20;
    let num_x = 20;
    let shift_y = 25.5;

    // NOTE: this spawns 11200 objects.
    for i in 0..num_x {
        for j in 0..num_z {
            let x = (i as f32 - num_x as f32 / 2.0) * (num_basis as f32 * 2.0 + 10.0);
            let z = (j as f32 - num_z as f32 / 2.0) * (num_basis as f32 * 2.0 + 10.0);
            create_wall(
                &mut result.bodies,
                &mut result.colliders,
                platform_shift + vector![x, shift_y, z],
                num_basis,
                vector![1.0, 0.5, 1.0],
            );
        }
    }
}

pub fn init_world() -> BuiltinScene {
    /*
     * World
     */
    let mut result = RapierContext::default();
    let mut animations = HashMap::default();

    // NOTE: there are 11.200 dynamic bodies per platform.
    // NOTE: count about 1000 dynamic bodies per core.
    let num_i = 3; // 5; // 8;
    let num_j = 1; // 6; // 8;

    /*
     * Create a floor to prevent objects from falling indefinitely.
     */
    {
        let rigid_body = RigidBodyBuilder::fixed().translation(Vector3::y() * 5.0);
        let ground_handle = result.bodies.insert(rigid_body);
        let collider = ColliderBuilder::halfspace(Vector3::y_axis());
        result
            .colliders
            .insert_with_parent(collider, ground_handle, &mut result.bodies);
    }

    /*
     * Create the platforms.
     */
    for i in 0..num_i {
        for j in 0..num_j {
            let shift = vector![
                GROUND_SIZE * 2.0 * std::f32::consts::SQRT_2 * (i as f32 - (num_i / 2) as f32),
                0.0,
                GROUND_SIZE * 2.0 * std::f32::consts::SQRT_2 * (j as f32 - (num_j / 2) as f32)
            ];
            init_platform_with_walls(&mut result, &mut animations, shift);
        }
    }

    BuiltinScene {
        context: result,
        animations,
    }
}
