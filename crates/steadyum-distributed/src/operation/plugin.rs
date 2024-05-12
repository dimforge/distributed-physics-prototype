use crate::operation::{self, Operations};
use crate::render::RenderSystems;
use bevy::prelude::*;

pub struct RapierOperationsPlugin;

impl Plugin for RapierOperationsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Operations::default())
            .add_systems(Last, clear_operations)
            .add_systems(
                Update,
                operation::import_scene
                    .after(operation::clear_scene)
                    .in_set(RenderSystems::ProcessCommands),
            )
            .add_systems(
                Update,
                operation::clear_scene.in_set(RenderSystems::ProcessCommands),
            );
    }
}

fn clear_operations(mut operations: ResMut<Operations>) {
    operations.clear();
}
