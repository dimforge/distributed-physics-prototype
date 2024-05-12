use bevy::prelude::*;
use steadyum_api_types::partitionner::SceneUuid;
use uuid::Uuid;

use crate::storage::SaveFileData;

pub enum Operation {
    ImportScene(SaveFileData),
    LoadNetworkScene(SceneUuid),
    ClearScene,
}

#[derive(Resource)]
pub struct Operations {
    stack: Vec<Operation>,
}

impl Default for Operations {
    fn default() -> Self {
        Self::new()
    }
}

impl Operations {
    pub fn new() -> Self {
        Self { stack: vec![] }
    }

    pub fn push(&mut self, command: Operation) {
        self.stack.push(command);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Operation> {
        self.stack.iter()
    }

    pub fn clear(&mut self) {
        self.stack.clear();
    }
}
