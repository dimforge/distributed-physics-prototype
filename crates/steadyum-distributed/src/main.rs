extern crate nalgebra as na;

#[cfg(feature = "dim2")]
extern crate rapier2d as rapier;
#[cfg(feature = "dim3")]
extern crate rapier3d as rapier;

use smooth_bevy_cameras::{
    controllers::unreal::{UnrealCameraBundle, UnrealCameraController, UnrealCameraPlugin},
    LookTransformPlugin,
};
use std::future::Future;

use crate::camera::OrbitCamera;
use crate::cli::CliArgs;
use crate::utils::RapierContext;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::camera::Projection;
use bevy::render::view::RenderLayers;
use bevy::utils::HashSet;
use bevy::winit::WinitWindows;
use clap::Parser;
use rapier::math::Real;
use steadyum_api_types::simulation::SimulationBounds;

mod camera;
// mod floor;
mod operation;
mod render;
mod styling;
mod ui;
mod utils;

mod storage;

mod builtin_scenes;
mod cli;

#[derive(Component)]
pub struct MainCamera;
#[derive(Component)]
pub struct GizmoCamera;

#[derive(Resource, Default)]
pub struct PhysicsProgress {
    pub simulated_time: Real,
    pub simulated_steps: usize,
    pub calculated_progress_limits_range: [u64; 2],
    pub progress_limit: usize,
    pub required_progress: u64,
    pub known_regions: HashSet<SimulationBounds>,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum SteadyumStages {
    PostPhysics,
    RenderStage,
}

fn main() {
    let args = CliArgs::parse();

    // let title = if cfg!(feature = "dim2") {
    //     "Steadyum 2D".to_string()
    // } else {
    //     "Steadyum 3D".to_string()
    // };

    let mut app = App::new();
    app
        /*.insert_resource(WindowDescriptor {
            title,
            ..Default::default()
        })*/
        .insert_resource(ClearColor(Color::rgb(0.55, 0.55, 0.55)))
        .insert_resource(args)
        .insert_resource(PhysicsProgress::default())
        .init_resource::<RapierContext>()
        .add_plugins(DefaultPlugins)
        .add_plugins(LogDiagnosticsPlugin::default())
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(bevy::pbr::wireframe::WireframePlugin)
        .add_plugins(bevy_obj::ObjPlugin)
        .add_plugins(LookTransformPlugin)
        .add_plugins(UnrealCameraPlugin::default())
        .add_plugins(render::RapierRenderPlugin)
        .add_plugins(ui::RapierUiPlugin)
        .add_plugins(styling::StylingPlugin)
        .add_plugins(operation::RapierOperationsPlugin)
        .add_systems(Startup, setup_graphics)
        .add_plugins(storage::StoragePlugin {
            local_dev_mode: args.dev,
        });

    app.run();
}

fn set_window_icon(windows: NonSendMut<WinitWindows>) {
    /*
    let primary = windows.get_window(WindowId::primary()).unwrap();

    // Here we use the `image` crate to load our icon data from a png file
    // this is not a very bevy-native solution, but it will do
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::open("assets/window_icon.png")
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };

    let icon = Icon::from_rgba(icon_rgba, icon_width, icon_height).unwrap();
    primary.set_window_icon(Some(icon));
     */
}

#[cfg(feature = "dim2")]
fn setup_graphics(mut commands: Commands) {
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 10_000.0,
            shadows_enabled: false,
            ..Default::default()
        },
        transform: Transform {
            translation: Vec3::new(10.0, 2.0, 10.0),
            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
            ..Default::default()
        },
        ..Default::default()
    });

    let orbit = OrbitCamera {
        pan_sensitivity: 0.01,
        ..OrbitCamera::default()
    };

    let camera = Camera2dBundle::default();
    commands
        .spawn(camera)
        .insert(orbit)
        .insert(MainCamera)
        .insert(RenderLayers::layer(0));
}

#[cfg(feature = "dim3")]
fn setup_graphics(mut commands: Commands) {
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 10_000.0,
            shadows_enabled: false,
            ..Default::default()
        },
        transform: Transform {
            translation: Vec3::new(10.0, 2.0, 10.0),
            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
            ..Default::default()
        },
        ..Default::default()
    });

    let mut orbit = OrbitCamera {
        pan_sensitivity: 4.0,
        rotate_sensitivity: 0.1,
        ..OrbitCamera::default()
    };
    look_at(&mut orbit, Vec3::new(5.0, 5.0, 5.0), Vec3::ZERO);
    commands
        .spawn(Camera3dBundle {
            transform: Transform::from_matrix(
                Mat4::look_at_rh(
                    Vec3::new(-3.0, 3.0, 1.0),
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                )
                .inverse(),
            ),
            projection: Projection::Perspective(PerspectiveProjection {
                far: 100.0,
                ..PerspectiveProjection::default()
            }),
            ..Default::default()
        })
        .insert(UnrealCameraBundle::new(
            UnrealCameraController { ..default() },
            Vec3::new(-2.0, 25.0, 5.0),
            Vec3::new(0., 25.0, 0.),
            Vec3::Y,
        ))
        // .insert(orbit)
        .insert(MainCamera)
        // .insert(GridShadowCamera)
        .insert(RenderLayers::layer(0));
}

#[cfg(feature = "dim2")]
pub fn look_at(camera: &mut OrbitCamera, at: Vec2, zoom: f32) {
    camera.center.x = at.x;
    camera.center.y = at.y;
    camera.zoom = zoom;
}

#[cfg(feature = "dim3")]
pub fn look_at(camera: &mut OrbitCamera, eye: Vec3, at: Vec3) {
    camera.center.x = at.x;
    camera.center.y = at.y;
    camera.center.z = at.z;

    let view_dir = eye - at;
    camera.distance = view_dir.length();

    if camera.distance > 0.0 {
        camera.y = (view_dir.y / camera.distance).acos();
        camera.x = (-view_dir.z).atan2(view_dir.x) - std::f32::consts::FRAC_PI_2;
    }
}

fn block_on<Fut: Future>(f: Fut) -> Fut::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}
