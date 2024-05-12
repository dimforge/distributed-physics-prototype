use bevy::prelude::*;
use bevy_egui::egui::TextureId;
use steadyum_api_types::partitionner::SceneUuid;

#[derive(Resource)]
pub struct UiState {
    pub button_texture_handles: Vec<Handle<Image>>,
    pub button_textures: Vec<TextureId>,
    pub network_scenes: Vec<SceneUuid>,
    pub debug_render_open: bool,
    pub simulation_infos_open: bool,
    pub single_step: bool,
    pub running: bool,
    pub interpolation: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            button_texture_handles: vec![],
            button_textures: vec![],
            network_scenes: vec![],
            debug_render_open: false,
            simulation_infos_open: false,
            single_step: false,
            running: false,
            interpolation: true,
        }
    }
}
