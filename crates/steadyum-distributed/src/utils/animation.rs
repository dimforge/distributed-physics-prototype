use bevy::prelude::Component;
use steadyum_api_types::kinematic::KinematicAnimations;

#[derive(Clone, Component)]
pub struct KinematicAnimationsComponent(pub KinematicAnimations);
