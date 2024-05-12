pub use self::animation::*;
pub use self::bevy_mesh_conversion::*;
pub use self::rapier_context::RapierContext;
use bevy::prelude::{Component, Transform};
use rapier::math::{Isometry, Real};
use uuid::Uuid;

mod animation;
mod bevy_mesh_conversion;
mod rapier_context;

#[cfg(feature = "dim2")]
pub type Vect = bevy::prelude::Vec2;
#[cfg(feature = "dim3")]
pub type Vect = bevy::prelude::Vec3;

#[derive(Component)]
pub struct PhysicsObject {
    pub uuid: Uuid,
    pub sleeping: bool,
}

#[derive(Copy, Clone, Component)]
pub struct MissingDataPoints(pub usize);

/// Converts a Rapier isometry to a Bevy transform.
///
/// The translation is multiplied by the `physics_scale`.
#[cfg(feature = "dim2")]
pub fn iso_to_transform(iso: &Isometry<Real>, physics_scale: Real) -> Transform {
    Transform {
        translation: (iso.translation.vector.push(0.0) * physics_scale).into(),
        rotation: bevy::prelude::Quat::from_rotation_z(iso.rotation.angle()),
        ..Default::default()
    }
}

/// Converts a Rapier isometry to a Bevy transform.
///
/// The translation is multiplied by the `physics_scale`.
#[cfg(feature = "dim3")]
pub fn iso_to_transform(iso: &Isometry<Real>, physics_scale: Real) -> Transform {
    Transform {
        translation: (iso.translation.vector * physics_scale).into(),
        rotation: iso.rotation.into(),
        ..Default::default()
    }
}

/// Converts a Bevy transform to a Rapier isometry.
///
/// The translation is divided by the `physics_scale`.
#[cfg(feature = "dim2")]
pub fn transform_to_iso(transform: &Transform, physics_scale: Real) -> Isometry<Real> {
    use bevy::math::Vec3Swizzles;
    Isometry::new(
        (transform.translation / physics_scale).xy().into(),
        transform.rotation.to_scaled_axis().z,
    )
}

/// Converts a Bevy transform to a Rapier isometry.
///
/// The translation is divided by the `physics_scale`.
#[cfg(feature = "dim3")]
pub fn transform_to_iso(transform: &Transform, physics_scale: Real) -> Isometry<Real> {
    Isometry::from_parts(
        (transform.translation / physics_scale).into(),
        transform.rotation.into(),
    )
}
