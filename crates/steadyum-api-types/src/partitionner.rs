use crate::messages::BodyAssignment;
use crate::region_db::AsyncPartitionnerServer;
use crate::simulation::SimulationBounds;
use rapier::geometry::Aabb;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const NUM_INTERNAL_STEPS: u64 = 10;

pub const SHUTDOWN: &str = "/shutdown";
pub const GET_EXES: &str = "/getbins";
pub const HEARTBEAT: &str = "/heartbeat";
pub const RUNNER_INITIALIZED_ENDPOINT: &str = "/initialized";
pub const ASSIGN_RUNNER_ENDPOINT: &str = "/region";
pub const INSERT_OBJECTS_ENDPOINT: &str = "/insert";
pub const LIST_REGIONS_ENDPOINT: &str = "/list_regions";
pub const LIST_SCENES_ENDPOINT: &str = "/list_scenes";
pub const START_STOP_ENDPOINT: &str = "/start_stop";
pub const CREATE_SCENE_ENDPOINT: &str = "/create_scene";
pub const REMOVE_SCENE_ENDPOINT: &str = "/remove_scene";
pub const REGISTER_CHILD_ENDPOINT: &str = "/register_child";
pub const ACK_ENDPOINT: &str = "/ack";
pub const STEP_ENDPOINT: &str = "/step";
pub const CLIENT_INPUT_ENDPOINT: &str = "/input";

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SceneUuid(pub Uuid);

impl Default for SceneUuid {
    fn default() -> Self {
        SceneUuid(Uuid::new_v4())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunnerInitializedRequest {
    pub scene: SceneUuid,
    pub uuid: Uuid,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssignRunnerRequest {
    pub scene: SceneUuid,
    pub region: SimulationBounds,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssignRunnerResponse {
    pub scene: SceneUuid,
    pub region: SimulationBounds,
    pub uuid: Uuid,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub scene: SceneUuid,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StepRequest {
    pub scene: SceneUuid,
    pub step_id: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClientInputRequest {
    pub scene: SceneUuid,
    pub step_id: u64,
    pub input: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InsertObjectsRequest {
    pub scene: SceneUuid,
    pub bodies: Vec<BodyAssignment>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ListRegionsRequest {
    pub scene: SceneUuid,
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct StartStopRequest {
    pub scene: SceneUuid,
    pub running: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ChildPartitionner {
    pub addr: String,
    pub port: u16,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RegisterChildRequest {
    pub child: ChildPartitionner,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CreateSceneRequest {
    pub scene: SceneUuid,
    pub bounds: Aabb,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CreateSceneResponse {
    pub runner: Uuid,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RemoveSceneRequest {
    pub scene: SceneUuid,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GetExesResponse {
    pub runner: Vec<u8>,
    pub partitionner: Vec<u8>,
}
