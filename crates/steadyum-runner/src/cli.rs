use steadyum_api_types::simulation::SimulationBounds;
use uuid::Uuid;

#[derive(clap::Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    #[arg(long)]
    pub uuid: u128,
    #[arg(long)]
    pub scene_uuid: u128,
    #[arg(long, default_value_t = 0)]
    pub time_origin: u64,
    #[arg(short, long, default_value_t = false)]
    pub dev: bool,
}

impl CliArgs {
    pub fn typed_uuid(&self) -> Uuid {
        Uuid::from_u128_le(self.uuid)
    }
    pub fn typed_scene_uuid(&self) -> Uuid {
        Uuid::from_u128_le(self.scene_uuid)
    }
}
