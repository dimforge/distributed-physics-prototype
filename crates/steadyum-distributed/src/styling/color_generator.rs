use bevy::prelude::*;
use std::collections::HashMap;
use steadyum_api_types::simulation::SimulationBounds;

#[derive(Resource)]
pub struct ColorGenerator {
    rng: oorandom::Rand32,
    region_colors: HashMap<SimulationBounds, Color>,
}

impl Default for ColorGenerator {
    fn default() -> Self {
        Self {
            rng: oorandom::Rand32::new(123456),
            region_colors: HashMap::new(),
        }
    }
}

impl ColorGenerator {
    pub fn gen_color(&mut self) -> Color {
        Color::rgb(
            self.rng.rand_float(),
            self.rng.rand_float(),
            self.rng.rand_float(),
        )
    }

    pub fn outline_color(color: Color) -> Color {
        if cfg!(feature = "dim2") {
            let [h, s, l, a] = color.as_hsla_f32();
            Color::hsla(h, s, l * 1.2, a)
        } else {
            color
        }
    }

    pub fn gen_region_color(&mut self, region: SimulationBounds) -> Color {
        let rng = &mut self.rng;
        *self
            .region_colors
            .entry(region)
            .or_insert_with(|| Color::rgb(rng.rand_float(), rng.rand_float(), rng.rand_float()))
    }

    pub fn static_object_color(&self) -> Color {
        Color::DARK_GREEN
    }
}
