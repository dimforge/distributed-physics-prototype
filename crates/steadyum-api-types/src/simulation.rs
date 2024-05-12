use crate::array_ser;
use crate::partitionner::SceneUuid;
use crate::zenoh::zenoh_storage_key;
use rapier::geometry::Aabb;
use rapier::math::{Point, Real, DIM};
use rapier::na::vector;
use rapier::parry::bounding_volume::BoundingVolume;
use std::cmp::Ordering;
use uuid::Uuid;

#[cfg(feature = "dim2")]
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    bytemuck::Zeroable,
    bytemuck::Pod,
)]
#[repr(transparent)]
pub struct SimulationBoundsU8 {
    #[serde(with = "array_ser")]
    bytes: [u8; 32],
}

#[cfg(feature = "dim3")]
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    bytemuck::Zeroable,
    bytemuck::Pod,
)]
#[repr(transparent)]
pub struct SimulationBoundsU8 {
    #[serde(with = "array_ser")]
    bytes: [u8; 48],
}

#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    bytemuck::Zeroable,
    bytemuck::Pod,
)]
#[repr(C)]
pub struct SimulationBounds {
    pub mins: [i64; DIM],
    pub maxs: [i64; DIM],
}

impl SimulationBounds {
    pub fn smallest() -> Self {
        Self {
            mins: [i64::MIN; DIM],
            maxs: [i64::MIN; DIM],
        }
    }
}

impl Default for SimulationBounds {
    fn default() -> Self {
        Self {
            mins: [-10_000; DIM],
            maxs: [10_000; DIM],
        }
    }
}

impl PartialOrd for SimulationBounds {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        for k in 0..DIM {
            if self.mins[k] < other.mins[k] {
                return Some(Ordering::Less);
            } else if self.mins[k] > other.mins[k] {
                return Some(Ordering::Greater);
            }
        }

        Some(Ordering::Equal)
    }
}

impl Ord for SimulationBounds {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl SimulationBounds {
    pub const DEFAULT_WIDTH: u64 = 100;

    pub fn from_aabb(aabb: &Aabb, region_width: u64) -> Self {
        Self::from_point(aabb.maxs, region_width)
    }

    pub fn from_point(point: Point<Real>, region_width: u64) -> Self {
        let mins = point
            .coords
            .map(|e| (e / region_width as Real).floor() as i64)
            * (region_width as i64);
        let maxs = mins.add_scalar(region_width as i64);

        Self {
            mins: mins.into(),
            maxs: maxs.into(),
        }
    }

    pub fn intersecting_aabb(aabb: Aabb, region_width: u64) -> Vec<Self> {
        let mut result = vec![];
        let min_region_id = aabb
            .mins
            .coords
            .map(|e| (e / region_width as Real).floor() as i64);
        let max_region_id = aabb
            .maxs
            .coords
            .map(|e| (e / region_width as Real).ceil() as i64);

        #[cfg(feature = "dim2")]
        for i in min_region_id.x..max_region_id.x {
            for j in min_region_id.y..max_region_id.y {
                let mins = vector![i, j] * region_width as i64;
                let maxs = mins.add_scalar(region_width as i64);
                result.push(Self {
                    mins: mins.into(),
                    maxs: maxs.into(),
                });
            }
        }

        #[cfg(feature = "dim3")]
        for i in min_region_id.x..max_region_id.x {
            for j in min_region_id.y..max_region_id.y {
                for k in min_region_id.z..max_region_id.z {
                    let mins = vector![i, j, k] * region_width as i64;
                    let maxs = mins.add_scalar(region_width as i64);
                    result.push(Self {
                        mins: mins.into(),
                        maxs: maxs.into(),
                    });
                }
            }
        }

        result
    }

    pub fn intersects_aabb(&self, aabb: &Aabb) -> bool {
        self.aabb().intersects(aabb)
    }

    pub fn as_bytes(&self) -> SimulationBoundsU8 {
        bytemuck::cast([self.mins, self.maxs])
    }

    pub fn aabb(&self) -> Aabb {
        Aabb {
            mins: Point::from(self.mins).cast::<Real>(),
            maxs: Point::from(self.maxs).cast::<Real>(),
        }
    }

    pub fn is_in_smaller_region(&self, aabb: &Aabb) -> bool {
        Self::from_aabb(aabb, Self::DEFAULT_WIDTH) < *self
    }

    pub fn intersects_master_region(&self, aabb: &Aabb) -> bool {
        Self::from_aabb(aabb, Self::DEFAULT_WIDTH) > *self
    }

    fn grid_extents(&self) -> [i64; 3] {
        [
            self.maxs[0] - self.mins[0],
            self.maxs[1] - self.mins[1],
            self.maxs[2] - self.mins[2],
        ]
    }

    pub fn relative_neighbor(&self, shift: [i64; 3]) -> Self {
        let extents = self.grid_extents();

        Self {
            mins: [
                self.mins[0] + shift[0] * extents[0],
                self.mins[1] + shift[1] * extents[1],
                self.mins[2] + shift[2] * extents[2],
            ],
            maxs: [
                self.maxs[0] + shift[0] * extents[0],
                self.maxs[1] + shift[1] * extents[1],
                self.maxs[2] + shift[2] * extents[2],
            ],
        }
    }

    #[cfg(feature = "dim2")]
    pub fn to_string(&self) -> String {
        format!(
            "{}_{}__{}_{}",
            self.mins[0], self.mins[1], self.maxs[0], self.maxs[1]
        )
    }

    #[cfg(feature = "dim3")]
    pub fn to_string(&self) -> String {
        format!(
            "{}_{}_{}__{}_{}_{}",
            self.mins[0], self.mins[1], self.mins[2], self.maxs[0], self.maxs[1], self.maxs[2]
        )
    }

    #[cfg(feature = "dim3")]
    pub fn from_str(str: &str) -> Option<Self> {
        use std::str::FromStr;

        let mut elts = str.split('_');
        let mins = [
            i64::from_str(elts.next()?).ok()?,
            i64::from_str(elts.next()?).ok()?,
            i64::from_str(elts.next()?).ok()?,
        ];
        elts.next()?;
        let maxs = [
            i64::from_str(elts.next()?).ok()?,
            i64::from_str(elts.next()?).ok()?,
            i64::from_str(elts.next()?).ok()?,
        ];
        Some(Self { mins, maxs })
    }

    pub fn zenoh_queue_key(&self, scene: SceneUuid) -> String {
        format!("runner/{:?}/{}", scene.0, self.to_string())
    }

    pub fn watch_kvs_key(&self, node: Uuid) -> String {
        format!("steadyum/watch/{:?}?{}", node, self.to_string())
    }

    pub fn runner_key(&self, scene: SceneUuid) -> String {
        self.zenoh_queue_key(scene)
    }

    pub fn runner_client_objects_key(&self, scene: SceneUuid, step_id: u64) -> String {
        format!(
            "steadyum/client_bodies/{:?}?{}&{}",
            scene.0,
            self.to_string(),
            step_id
        )
    }

    #[cfg(feature = "dim2")]
    pub fn neighbors_to_watch(&self) -> [Self; 3] {
        let mut result = [*self; 3];
        let mut curr = 0;

        for i in 0..=1 {
            for j in 0..=1 {
                if i == 0 && j == 0 {
                    continue; // Exclude self.
                }

                let width = [
                    (self.maxs[0] - self.mins[0]) * i,
                    (self.maxs[1] - self.mins[1]) * j,
                ];

                let adj_region = Self {
                    mins: [self.mins[0] + width[0], self.mins[1] + width[1]],
                    maxs: [self.maxs[0] + width[0], self.maxs[1] + width[1]],
                };

                result[curr] = adj_region;
                curr += 1;
            }
        }

        result
    }

    #[cfg(feature = "dim3")]
    pub fn neighbors_to_watch(&self) -> [Self; 7] {
        let mut result = [*self; 7];
        let mut curr = 0;

        // NOTE: in some very specific corner cases, we might want
        //       to grab the watch sets from every neighbor greater than
        //       `self`.
        for i in 0..=1 {
            for j in 0..=1 {
                for k in 0..=1 {
                    if i == 0 && j == 0 && k == 0 {
                        continue; // Exclude self.
                    }

                    let width = [
                        (self.maxs[0] - self.mins[0]) * i,
                        (self.maxs[1] - self.mins[1]) * j,
                        (self.maxs[2] - self.mins[2]) * k,
                    ];

                    let adj_region = Self {
                        mins: [
                            self.mins[0] + width[0],
                            self.mins[1] + width[1],
                            self.mins[2] + width[2],
                        ],
                        maxs: [
                            self.maxs[0] + width[0],
                            self.maxs[1] + width[1],
                            self.maxs[2] + width[2],
                        ],
                    };

                    if adj_region > *self {
                        result[curr] = adj_region;
                        curr += 1;
                    }
                }
            }
        }

        assert_eq!(curr, 7);

        result
    }

    #[cfg(feature = "dim3")]
    pub fn all_neighbors(&self) -> [Self; 26] {
        let mut result = [*self; 26];
        let mut curr = 0;

        for i in -1..=1 {
            for j in -1..=1 {
                for k in -1..=1 {
                    if i == 0 && j == 0 && k == 0 {
                        continue; // Exclude self.
                    }

                    let width = [
                        (self.maxs[0] - self.mins[0]) * i,
                        (self.maxs[1] - self.mins[1]) * j,
                        (self.maxs[2] - self.mins[2]) * k,
                    ];

                    let adj_region = Self {
                        mins: [
                            self.mins[0] + width[0],
                            self.mins[1] + width[1],
                            self.mins[2] + width[2],
                        ],
                        maxs: [
                            self.maxs[0] + width[0],
                            self.maxs[1] + width[1],
                            self.maxs[2] + width[2],
                        ],
                    };

                    result[curr] = adj_region;
                    curr += 1;
                }
            }
        }

        assert_eq!(curr, 26);

        result
    }
}
