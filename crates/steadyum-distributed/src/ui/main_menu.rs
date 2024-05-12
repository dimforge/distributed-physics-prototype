use crate::operation::{Operation, Operations};
use crate::storage::{DbContext, SaveFileData};
use crate::styling::Theme;
use crate::ui::UiState;
use crate::{block_on, builtin_scenes};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::window::Window;
use bevy_egui::{egui, EguiContexts};

use crate::utils::RapierContext;
#[cfg(not(target_arch = "wasm32"))]
use native_dialog::FileDialog;
use steadyum_api_types::region_db::AsyncPartitionnerServer;

pub(super) fn ui(
    _window: &Window,
    theme: &mut Theme,
    ui_context: &mut EguiContexts,
    ui_state: &mut UiState,
    operations: &mut Operations,
    partitionner: &AsyncPartitionnerServer,
    mut exit: EventWriter<AppExit>,
) {
    egui::Window::new("main menu")
        .resizable(false)
        .title_bar(false)
        .fixed_pos([5.0, 5.0])
        .show(ui_context.ctx_mut(), |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("📁 Open…").clicked() {
                        match import_data::<RapierContext>() {
                            Ok(Some(scene)) => {
                                operations.push(Operation::ClearScene);
                                operations.push(Operation::ImportScene(scene))
                            }
                            Ok(None) => {}
                            Err(e) => error!("Failed to import scene: {:?}", e),
                        }
                    }

                    ui.menu_button("📂 Built-in scenes", |ui| {
                        for (name, builder) in builtin_scenes::builders() {
                            if ui.button(name).clicked() {
                                let ctxt = builder();
                                operations.push(Operation::ClearScene);
                                operations.push(Operation::ImportScene(SaveFileData::from(ctxt)));
                            }
                        }
                    });

                    ui.menu_button("Network scenes", |ui| {
                        for uuid in &ui_state.network_scenes {
                            if ui.button(format!("{}", uuid.0)).clicked() {
                                operations.push(Operation::LoadNetworkScene(*uuid));
                            }
                        }

                        if ui.button("Reload list…").clicked() {
                            block_on(async {
                                if let Ok(list) = partitionner.list_scenes().await {
                                    ui_state.network_scenes = list.scenes;
                                }
                            });
                        }
                    });

                    ui.checkbox(&mut theme.dark_mode, "Dark mode");

                    if ui.button("ℹ Simulation infos…").clicked() {
                        ui_state.simulation_infos_open = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("❌ Clear scene").clicked() {
                        operations.push(Operation::ClearScene)
                    }
                    if ui.button("🚪 Exit").clicked() {
                        exit.send(AppExit);
                    }
                });
            })
        });
}

#[cfg(not(target_arch = "wasm32"))]
fn import_data<T: serde::Serialize>() -> anyhow::Result<Option<SaveFileData>> {
    if let Some(path) = FileDialog::new()
        .add_filter("Json", &["json"])
        .show_open_single_file()?
    {
        let data = std::fs::read(path)?;
        Ok(serde_json::from_slice(&data)?)
    } else {
        Ok(None)
    }
}
