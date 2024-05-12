use crate::builtin_scenes::BuiltinScene;
use crate::operation::clear_scene;
use crate::render::RenderSystems;
use bevy::prelude::*;
use rapier::prelude::{GenericJoint, RigidBodyHandle};
use steadyum_api_types::objects::{ColdBodyObject, WarmBodyObject};
use uuid::Uuid;

pub struct StoragePlugin {
    pub local_dev_mode: bool,
}

#[cfg(target_arch = "wasm32")]
impl Plugin for StoragePlugin {
    fn build(&self, app: &mut App) {}
}

#[cfg(not(target_arch = "wasm32"))]
impl Plugin for StoragePlugin {
    fn build(&self, app: &mut App) {
        use super::systems;

        let context = super::db::spawn_db_thread(self.local_dev_mode);
        app.insert_resource(context)
            .add_systems(PreUpdate, systems::read_object_positions_from_kvs)
            .add_systems(PreUpdate, systems::update_start_stop)
            .add_systems(Update, systems::update_camera_pos)
            .add_systems(Update, systems::step_interpolations)
            .add_systems(Update, systems::update_physics_progress)
            .add_systems(Update, systems::integrate_kinematic_animations)
            .add_systems(Last, systems::emit_client_inputs)
            .add_systems(Last, systems::remove_scene_on_exit)
            .add_systems(
                Update,
                systems::handle_scene_reset
                    .before(clear_scene)
                    .in_set(RenderSystems::ProcessCommands),
            )
            .add_systems(Update, systems::open_existing_scene);
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SaveFileData {
    pub objects: Vec<(RigidBodyHandle, ColdBodyObject, WarmBodyObject)>,
    pub impulse_joints: Vec<(RigidBodyHandle, RigidBodyHandle, GenericJoint)>,
}

impl From<BuiltinScene> for SaveFileData {
    fn from(scene: BuiltinScene) -> Self {
        let mut result = SaveFileData::default();

        for (_, collider) in scene.context.colliders.iter() {
            let parent = collider
                .parent()
                .expect("Parentless colliders are not supported yet.");
            let body = &scene.context.bodies[parent];
            let warm_object = WarmBodyObject::from_body(body, 0);
            let mut cold_object = ColdBodyObject::from_body_collider(body, collider);
            if let Some(animations) = scene.animations.get(&parent) {
                cold_object.animations = animations.clone();
            }
            result.objects.push((parent, cold_object, warm_object));
        }

        for (_, joint) in scene.context.impulse_joints.iter() {
            result
                .impulse_joints
                .push((joint.body1, joint.body2, joint.data));
        }

        result
    }
}
