use crate::kinematic::KinematicAnimations;
use crate::partitionner::SceneUuid;
use crate::simulation::SimulationBounds;
use rapier::math::{AngVector, Isometry, Real, Vector};
use rapier::prelude::{Aabb, Collider, ColliderShape, RigidBody, RigidBodyType};
use uuid::Uuid;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ClientBodyObject {
    pub uuid: Uuid,
    pub position: Isometry<Real>,
    // TODO: a bit sad to always re-send the shape.
    //       Needs to be benchmarked to determine if it’s to slow (probably will be whenever we don’t use a complex shape).
    pub shape: ColliderShape,
    pub body_type: RigidBodyType,
    pub sleep_start_frame: Option<u64>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ClientBodyObjectSet {
    pub timestamp: u64,
    pub objects: Vec<ClientBodyObject>,
}

#[derive(Copy, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WarmBodyObject {
    pub timestamp: u64,
    pub position: Isometry<Real>,
    pub linvel: Vector<Real>,
    pub angvel: AngVector<Real>,
}

impl WarmBodyObject {
    pub fn from_body(body: &RigidBody, timestamp: u64) -> Self {
        Self {
            timestamp,
            position: *body.position(),
            linvel: *body.linvel(),
            angvel: body.angvel().clone(),
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ColdBodyObject {
    pub body_type: RigidBodyType,
    pub density: Real,
    pub shape: ColliderShape,
    pub animations: KinematicAnimations,
}

impl ColdBodyObject {
    pub fn from_body_collider(body: &RigidBody, collider: &Collider) -> Self {
        Self {
            body_type: body.body_type(),
            density: collider.density(),
            shape: collider.shared_shape().clone(),
            animations: KinematicAnimations::default(),
        }
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WatchedObjects {
    pub objects: Vec<(Uuid, Aabb)>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Default, Debug)]
pub struct RegionList {
    pub bounds: Vec<SimulationBounds>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Default, Debug)]
pub struct SceneList {
    pub scenes: Vec<SceneUuid>,
}
