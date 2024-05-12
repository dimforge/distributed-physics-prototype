use bevy::prelude::Component;
use rapier::math::{Isometry, Real};
use std::collections::VecDeque;

#[derive(Copy, Clone, Debug, Default)]
struct PositionInterpolationPoint {
    pub pos: Isometry<Real>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Component)]
pub struct PositionInterpolation {
    current: PositionInterpolationPoint,
    targets: VecDeque<PositionInterpolationPoint>,
}

impl PositionInterpolation {
    pub fn new(pos: Isometry<Real>, timestamp: u64) -> Self {
        Self {
            current: PositionInterpolationPoint { pos, timestamp },
            targets: VecDeque::new(),
        }
    }
}

impl PositionInterpolation {
    pub fn step(&mut self, timestamp: u64) {
        while !self.targets.is_empty() {
            if self.targets[0].timestamp <= timestamp {
                self.current = self.targets.pop_front().unwrap();
            } else {
                break;
            }
        }

        // Now, interpolate between the current pos and the target pos.
        if !self.targets.is_empty() {
            let target = &self.targets[0];
            let t = (timestamp as Real - self.current.timestamp as Real).max(0.0)
                / (target.timestamp as Real - self.current.timestamp as Real);
            self.current.pos = self.current.pos.lerp_slerp(&target.pos, t);
            self.current.timestamp = timestamp;
        }
    }

    pub fn current_pos(&self) -> Isometry<Real> {
        self.current.pos
    }

    pub fn final_pos(&self) -> &Isometry<Real> {
        self.targets
            .back()
            .map(|p| &p.pos)
            .unwrap_or(&self.current.pos)
    }

    pub fn max_known_timestep(&self) -> u64 {
        self.targets
            .back()
            .map(|p| p.timestamp)
            .unwrap_or(self.current.timestamp)
    }

    pub fn add_interpolation_point(&mut self, pos: Isometry<Real>, timestamp: u64) {
        // TODO: don’t accumulate interpolation point with equal positions, or with
        //       position that could be part of the interpolation.
        self.targets
            .push_back(PositionInterpolationPoint { pos, timestamp });
    }
}
