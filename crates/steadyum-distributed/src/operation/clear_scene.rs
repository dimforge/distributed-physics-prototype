use crate::operation::{Operation, Operations};
use crate::utils::PhysicsObject;
use crate::PhysicsProgress;
use bevy::prelude::*;

pub fn clear_scene(
    mut commands: Commands,
    mut progress: ResMut<PhysicsProgress>,
    operations: Res<Operations>,
    to_remove: Query<Entity, With<PhysicsObject>>,
) {
    for op in operations.iter() {
        if let Operation::ClearScene = op {
            dbg!("Clearing scene.");
            progress.simulated_time = 0.0;
            for entity in to_remove.iter() {
                commands.entity(entity).despawn_recursive();
            }
        }
    }
}
