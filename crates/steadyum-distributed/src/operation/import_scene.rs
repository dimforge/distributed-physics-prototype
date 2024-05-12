use crate::operation::{Operation, Operations};
use crate::storage::{DbCommand, DbContext, NewObjectCommand};
use bevy::prelude::*;
use rapier::prelude::RigidBodyHandle;
use uuid::Uuid;

pub fn import_scene(operations: Res<Operations>, db_context: Res<DbContext>) {
    for op in operations.iter() {
        if let Operation::ImportScene(scene) = op {
            info!("Importing {} bodies to the scene.", scene.objects.len());
            let objects = scene
                .objects
                .iter()
                .map(
                    |(handle_or_uuid, cold_object, warm_object)| NewObjectCommand {
                        uuid: Uuid::new_v4(),
                        handle: RigidBodyHandle::invalid(),
                        cold_object: cold_object.clone(),
                        warm_object: warm_object.clone(),
                    },
                )
                .collect();
            if let Err(e) = db_context
                .commands_snd
                .send_blocking(DbCommand::NewScene { objects })
            {
                error!("Failed to send new object to DB: {e}");
            }
        }
    }
}
