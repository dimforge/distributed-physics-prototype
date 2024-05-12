use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{
    egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText},
    EguiContexts,
};
use image::Progress;
use strum_macros::EnumIter;

pub use self::plugin::RapierUiPlugin;
use crate::cli::CliArgs;
use crate::operation::Operations;
use crate::storage::DbContext;
use crate::styling::Theme;
use crate::utils::{PhysicsObject, RapierContext};
use crate::PhysicsProgress;
pub use ui_state::UiState;

// mod gizmo;
mod main_menu;
mod play_stop;
mod plugin;
mod popup_menu;
mod simulation_infos;
mod ui_state;

#[derive(Copy, Clone, Debug, PartialEq, Eq, EnumIter)]
pub enum ButtonTexture {
    Play,
    Pause,
    Undo,
    Redo,
}

impl ButtonTexture {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Play => "",
            Self::Pause => "",
            Self::Undo => "",
            Self::Redo => "",
        }
    }

    pub fn rich_text(self) -> RichText {
        let txt = egui::RichText::new(self.icon());

        match self {
            Self::Play => txt
                .color(Color32::LIGHT_GREEN)
                .font(egui::FontId::monospace(40.0).clone()),
            Self::Pause => txt
                .color(Color32::LIGHT_RED)
                .font(egui::FontId::monospace(40.0).clone()),
            Self::Undo | Self::Redo => txt
                .color(Color32::LIGHT_BLUE)
                .font(egui::FontId::monospace(40.0).clone()),
        }
    }
}

pub fn load_assets(
    mut ui_context: EguiContexts,
    _ui_state: ResMut<UiState>,
    _assets: Res<AssetServer>,
) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "blender-icons".to_owned(),
        FontData::from_static(include_bytes!("../../../../assets/blender-icons.ttf")),
    );
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .push("blender-icons".to_owned());
    ui_context.ctx_mut().set_fonts(fonts);
}

pub fn update_ui(
    mut commands: Commands,
    (cli, mut theme): (Res<CliArgs>, ResMut<Theme>),
    mut ui_context: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut physics_context: ResMut<RapierContext>,
    mut operations: ResMut<Operations>,
    progress: Res<PhysicsProgress>,
    db_ctxt: Res<DbContext>,
    exit: EventWriter<AppExit>,
    windows: Query<&Window, With<PrimaryWindow>>,
    visible_objects: Query<&InheritedVisibility, With<PhysicsObject>>,
) {
    if let Ok(window) = windows.get_single() {
        main_menu::ui(
            window,
            &mut theme,
            &mut ui_context,
            &mut ui_state,
            &mut *operations,
            &db_ctxt.partitionner,
            exit,
        );
        play_stop::ui(
            window,
            &cli,
            &mut ui_context,
            &mut ui_state,
            &mut *physics_context,
        );
        popup_menu::ui(window, &mut ui_context, &mut *physics_context);

        let num_visible_objects = visible_objects.iter().filter(|vis| vis.get()).count();
        simulation_infos::ui(
            &mut ui_context,
            &mut ui_state,
            &*physics_context,
            &*progress,
            &db_ctxt.stats,
            num_visible_objects,
        );
    }
}
