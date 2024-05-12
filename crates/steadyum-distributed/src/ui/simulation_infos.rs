use crate::storage::DbStats;
use crate::ui::UiState;
use crate::utils::RapierContext;
use crate::PhysicsProgress;
use bevy_egui::{egui, EguiContexts};

pub(super) fn ui(
    ui_context: &mut EguiContexts,
    ui_state: &mut UiState,
    physics: &RapierContext,
    progress: &PhysicsProgress,
    db_stats: &DbStats,
    num_visible_objects: usize,
) {
    egui::Window::new("ℹ Simulation infos")
        .open(&mut ui_state.simulation_infos_open)
        .resizable(false)
        .show(ui_context.ctx_mut(), |ui| {
            ui.label(stats_string(
                physics,
                progress,
                db_stats,
                num_visible_objects,
            ));
        });
}

fn stats_string(
    physics: &RapierContext,
    progress: &PhysicsProgress,
    db_stats: &DbStats,
    num_visible_objects: usize,
) -> String {
    format!(
        r#"Visible objects: {}
Progress limits range: {:?}
curr step: {}/{}
{:#?}"#,
        num_visible_objects,
        progress.calculated_progress_limits_range,
        progress.simulated_steps,
        progress.progress_limit,
        db_stats
    )
}
