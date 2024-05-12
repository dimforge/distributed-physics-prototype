use crate::env::CONFIG;
use crate::messages::BodyAssignment;
use crate::objects::{RegionList, SceneList};
use crate::partitionner::{
    AckRequest, ClientInputRequest, CreateSceneRequest, CreateSceneResponse, StepRequest,
    CLIENT_INPUT_ENDPOINT, CREATE_SCENE_ENDPOINT, LIST_SCENES_ENDPOINT, STEP_ENDPOINT,
};
use crate::partitionner::{
    AssignRunnerRequest, AssignRunnerResponse, ChildPartitionner, GetExesResponse,
    InsertObjectsRequest, ListRegionsRequest, RegisterChildRequest, RemoveSceneRequest,
    RunnerInitializedRequest, SceneUuid, StartStopRequest, ACK_ENDPOINT, ASSIGN_RUNNER_ENDPOINT,
    GET_EXES, HEARTBEAT, INSERT_OBJECTS_ENDPOINT, LIST_REGIONS_ENDPOINT, REGISTER_CHILD_ENDPOINT,
    REMOVE_SCENE_ENDPOINT, RUNNER_INITIALIZED_ENDPOINT, SHUTDOWN, START_STOP_ENDPOINT,
};
use crate::serialization::deserialize;
use crate::simulation::SimulationBounds;
use rapier::prelude::Aabb;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct AsyncPartitionnerServer {
    client: reqwest::Client,
    addr: String,
    port: u16,
}

impl AsyncPartitionnerServer {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_endpoint(CONFIG.partitionner_addr.clone(), CONFIG.partitionner_port)
    }

    pub fn with_endpoint(addr: String, port: u16) -> anyhow::Result<Self> {
        let client = reqwest::Client::new();
        Ok(Self { client, addr, port })
    }

    pub fn local() -> anyhow::Result<Self> {
        Self::with_endpoint("http://localhost".to_string(), CONFIG.partitionner_port)
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.client.get(self.endpoint(SHUTDOWN)).send().await?;
        Ok(())
    }

    pub async fn heartbeat(&self) -> anyhow::Result<()> {
        self.client
            .get(self.endpoint(HEARTBEAT))
            .timeout(Duration::from_secs(2))
            .send()
            .await?;
        Ok(())
    }

    pub async fn get_exes(&self) -> anyhow::Result<GetExesResponse> {
        let raw_response = self
            .client
            .get(self.endpoint(GET_EXES))
            .timeout(Duration::from_secs(2))
            .send()
            .await?;
        let response = deserialize(&raw_response.bytes().await?)?;
        Ok(response)
    }

    pub async fn put_runner_initialized(&self, scene: SceneUuid, uuid: Uuid) -> anyhow::Result<()> {
        let body = RunnerInitializedRequest { scene, uuid };
        self.client
            .post(self.endpoint(RUNNER_INITIALIZED_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    pub async fn allocate_runner(
        &self,
        scene: SceneUuid,
        region: SimulationBounds,
    ) -> anyhow::Result<Uuid> {
        let body = AssignRunnerRequest { scene, region };
        let raw_response = self
            .client
            .post(self.endpoint(ASSIGN_RUNNER_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        let response: AssignRunnerResponse = raw_response.json().await?;
        Ok(response.uuid)
    }

    pub async fn insert_objects(
        &self,
        scene: SceneUuid,
        bodies: Vec<BodyAssignment>,
    ) -> anyhow::Result<()> {
        let body = InsertObjectsRequest { scene, bodies };
        self.client
            .post(self.endpoint(INSERT_OBJECTS_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    pub async fn list_regions(&self, scene: SceneUuid) -> anyhow::Result<RegionList> {
        let body = ListRegionsRequest { scene };
        let raw_response = self
            .client
            .get(self.endpoint(LIST_REGIONS_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(raw_response.json().await?)
    }

    pub async fn list_scenes(&self) -> anyhow::Result<SceneList> {
        let raw_response = self
            .client
            .get(self.endpoint(LIST_SCENES_ENDPOINT))
            .send()
            .await?;
        Ok(raw_response.json().await?)
    }

    pub async fn set_running(&self, scene: SceneUuid, running: bool) -> anyhow::Result<()> {
        let body = StartStopRequest { scene, running };
        self.client
            .post(self.endpoint(START_STOP_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    pub async fn register_child(&self, child: ChildPartitionner) -> anyhow::Result<()> {
        let body = RegisterChildRequest { child };
        self.client
            .post(self.endpoint(REGISTER_CHILD_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    pub async fn create_scene(
        &self,
        scene: SceneUuid,
        bounds: Aabb,
    ) -> anyhow::Result<CreateSceneResponse> {
        let body = CreateSceneRequest { scene, bounds };
        let raw_response = self
            .client
            .post(self.endpoint(CREATE_SCENE_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(raw_response.json().await?)
    }

    pub fn create_scene_blocking(
        &self,
        scene: SceneUuid,
        bounds: Aabb,
    ) -> anyhow::Result<CreateSceneResponse> {
        tokio::runtime::Builder::new_current_thread()
            .build()?
            .block_on(self.create_scene(scene, bounds))
    }

    pub async fn remove_scene(&self, scene: SceneUuid) -> anyhow::Result<()> {
        let body = RemoveSceneRequest { scene };
        self.client
            .post(self.endpoint(REMOVE_SCENE_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    pub fn remove_scene_blocking(&self, scene: SceneUuid) -> anyhow::Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .build()?
            .block_on(self.remove_scene(scene))
    }

    pub async fn client_input(&self, scene: SceneUuid, step_id: u64) -> anyhow::Result<()> {
        let body = ClientInputRequest {
            scene,
            step_id,
            input: 0,
        };
        self.client
            .post(self.endpoint(CLIENT_INPUT_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    pub fn client_input_blocking(&self, scene: SceneUuid, step_id: u64) -> anyhow::Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .build()?
            .block_on(self.client_input(scene, step_id))
    }

    pub async fn ack(&self, scene: SceneUuid) -> anyhow::Result<()> {
        let body = AckRequest { scene };
        self.client
            .post(self.endpoint(ACK_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    pub async fn step(&self, scene: SceneUuid, step_id: u64) -> anyhow::Result<()> {
        let body = StepRequest { scene, step_id };
        self.client
            .post(self.endpoint(STEP_ENDPOINT))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    fn endpoint(&self, endpoint: &str) -> String {
        format!("{}:{}{endpoint}", self.addr, self.port)
    }
}
