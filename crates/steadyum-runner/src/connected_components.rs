use crate::runner::SimulationState;
use rapier::parry::bounding_volume::BoundingVolume;
use rapier::prelude::*;
use std::collections::HashSet;

#[derive(Clone)]
pub struct ConnectedComponent {
    pub bodies: Vec<RigidBodyHandle>,
    pub joints: Vec<(
        RigidBodyHandle,
        RigidBodyHandle,
        GenericJoint,
        ImpulseJointHandle,
    )>,
    pub swept_aabb: Aabb,
}

impl Default for ConnectedComponent {
    fn default() -> Self {
        Self {
            bodies: vec![],
            joints: vec![],
            swept_aabb: Aabb::new_invalid(),
        }
    }
}

pub fn calculate_connected_components(
    sim_state: &SimulationState,
    num_steps_run: usize,
) -> Vec<ConnectedComponent> {
    let mut visited = HashSet::new();
    let mut visited_joints = HashSet::new();
    let mut stack = vec![];
    let mut connected_bodies = vec![];
    let mut connected_joints = vec![];
    let mut result = vec![];
    let mut swept_aabb = Aabb::new_invalid();

    for (handle, _body) in sim_state.bodies.iter() {
        stack.push(handle);

        while let Some(body_handle) = stack.pop() {
            if visited.contains(&body_handle) {
                continue;
            }

            let body = &sim_state.bodies[body_handle];
            if !body.is_dynamic() {
                continue;
            }

            visited.insert(body_handle);
            connected_bodies.push(body_handle);

            for collider_handle in sim_state.bodies[body_handle].colliders() {
                let collider = &sim_state.colliders[*collider_handle];
                let predicted_pos = body.predict_position_using_velocity_and_forces(
                    sim_state.params.dt * num_steps_run as f32,
                );
                swept_aabb.merge(&collider.compute_swept_aabb(&predicted_pos));

                for contact in sim_state.narrow_phase.contacts_with(*collider_handle) {
                    let other_collider_handle = if contact.collider1 == *collider_handle {
                        contact.collider2
                    } else {
                        contact.collider1
                    };

                    if let Some(parent_handle) = sim_state.colliders[other_collider_handle].parent()
                    {
                        stack.push(parent_handle);
                    }
                }
            }

            for (rb1, rb2, joint_handle, joint) in
                sim_state.impulse_joints.attached_joints(body_handle)
            {
                let other_body_handle = if rb1 == body_handle { rb2 } else { rb1 };

                if visited_joints.insert(joint_handle) {
                    connected_joints.push((rb1, rb2, joint.data, joint_handle));
                }
                stack.push(other_body_handle);
            }
        }

        let cc = ConnectedComponent {
            bodies: std::mem::replace(&mut connected_bodies, vec![]),
            joints: std::mem::replace(&mut connected_joints, vec![]),
            swept_aabb,
        };

        if !cc.bodies.is_empty() || !cc.joints.is_empty() {
            result.push(cc);
        }
    }

    result
}
