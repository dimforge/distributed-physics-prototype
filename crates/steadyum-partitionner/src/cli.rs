use steadyum_api_types::simulation::SimulationBounds;
use uuid::Uuid;

#[derive(clap::Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    #[arg(short, long, default_value_t = false)]
    pub runner: bool,
    #[arg(short, long, default_value_t = false)]
    pub dev: bool,
}
