use crate::render::CollisionShapeMeshInstances;
use bevy::prelude::*;

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum RenderSystems {
    ProcessCommands,
    AddMissingTransforms,
    CreateColliderRenders,
    CreateColliderOutlineRenders,
    RenderJoints,
}

/// Plugin responsible for creating meshes to render the Rapier physics scene.
pub struct RapierRenderPlugin;

impl Plugin for RapierRenderPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Update,
            (
                RenderSystems::ProcessCommands,
                RenderSystems::AddMissingTransforms,
                RenderSystems::CreateColliderRenders,
                RenderSystems::CreateColliderOutlineRenders,
                RenderSystems::RenderJoints,
            )
                .chain(),
        );
        // app.add_stage_before(
        //     Update,
        //     SteadyumStages::RenderStage,
        //     SystemStage::parallel(),
        // );

        app // .add_plugins(bevy_prototype_debug_lines::DebugLinesPlugin::with_depth_test(false))
            .init_resource::<CollisionShapeMeshInstances>()
            .add_systems(
                Update,
                apply_deferred
                    .after(RenderSystems::AddMissingTransforms)
                    .before(RenderSystems::CreateColliderRenders),
            )
            .add_systems(
                Update, // SteadyumStages::RenderStage,
                super::create_collider_renders_system.in_set(RenderSystems::CreateColliderRenders),
            );

        // .add_systems(
        //     CoreStage::Update,
        //     super::render_joints.label(RenderSystems::RenderJoints),
        // );

        #[cfg(feature = "dim2")]
        {
            app.add_plugins(bevy_prototype_lyon::prelude::ShapePlugin);
        }

        // #[cfg(feature = "dim3")]
        // {
        //     app.add_plugins(bevy_polyline::PolylinePlugin);
        // }
    }
}
