use crate::render::{ColliderRender, ColliderRenderShape};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, VertexAttributeValues};
use na::{point, UnitQuaternion};
use rapier::math::{Point, Real, Vector};

use crate::cli::CliArgs;
#[cfg(feature = "dim2")]
use bevy::sprite::MaterialMesh2dBundle;
use bevy_egui::egui::ahash::HashMap;
use rapier::geometry::ColliderShape;
use rapier::parry::shape::Cuboid;
use rapier::prelude::{Collider, TypedShape};

#[derive(Resource, Default, Clone)]
pub struct CollisionShapeMeshInstances {
    cuboid_to_mesh: Vec<(Cuboid, Handle<Mesh>)>, // TODO: make this work for all collider types.
    color_to_material: Vec<(Color, Handle<StandardMaterial>)>,
}

#[derive(Component, Copy, Clone)]
pub struct RenderInitialized;

/// System responsible for attaching a PbrBundle to each entity having a collider.
pub fn create_collider_renders_system(
    mut commands: Commands,
    cli: Res<CliArgs>,
    mut instances: ResMut<CollisionShapeMeshInstances>,
    mut meshes: ResMut<Assets<Mesh>>,
    #[cfg(feature = "dim2")] mut materials: ResMut<Assets<ColorMaterial>>,
    #[cfg(feature = "dim3")] mut materials: ResMut<Assets<StandardMaterial>>,
    mut coll_shape_render: Query<
        (Entity, &Transform, &ColliderRenderShape, &ColliderRender),
        Or<(
            Changed<ColliderRenderShape>,
            Changed<ColliderRender>,
            Without<RenderInitialized>,
        )>,
    >,
) {
    for (entity, transform, collider, render) in coll_shape_render.iter_mut() {
        commands.entity(entity).insert(RenderInitialized); // FIXME: not sure what this is needed. Change detection for Changed<ColliderRender> should be enough.
        if let Some(mesh) =
            generate_collision_shape_render_mesh(&collider.shape, &mut *meshes, &mut instances)
        {
            let mut material: StandardMaterial = render.color.into();
            material.double_sided = true;

            let material_handle = instances
                .color_to_material
                .iter()
                .find(|(c, _)| *c == render.color)
                .map(|(_, m)| m.clone())
                .unwrap_or_else(|| materials.add(material));

            // println!("Rendering with color: {:?}", render.color);

            #[cfg(feature = "dim2")]
            {
                if let TypedShape::Cuboid(s) = collider.as_unscaled_typed_shape() {
                    #[cfg(feature = "dim2")]
                    let mut bundle = SpriteBundle {
                        sprite: Sprite {
                            color: render.color.into(),
                            custom_size: Some(Vec2::new(
                                s.half_extents.x * 2.0,
                                s.half_extents.y * 2.0,
                            )),
                            ..default()
                        },
                        ..default()
                    };

                    bundle.transform = *transform;
                    commands.entity(entity).insert(bundle);
                    continue;
                }
            }

            #[cfg(feature = "dim2")]
            let mut bundle = MaterialMesh2dBundle {
                mesh: mesh.into(),
                material: materials.add(render.color.into()),
                transform: Transform::from_xyz(0.0, 0.0, (entity.index() + 1) as f32 * 1.0001e-9),
                ..Default::default()
            };

            #[cfg(feature = "dim3")]
            let mut bundle = PbrBundle {
                mesh,
                material: material_handle,
                ..Default::default()
            };

            bundle.transform = *transform;
            commands.entity(entity).insert(bundle);
        }
    }
}

#[cfg(feature = "dim3")]
fn generate_collision_shape_render_mesh(
    shape: &ColliderShape,
    meshes: &mut Assets<Mesh>,
    instances: &mut CollisionShapeMeshInstances,
) -> Option<Handle<Mesh>> {
    const NSUB: u32 = 20;

    let ((vertices, indices), flat_normals) = match shape.as_typed_shape() {
        TypedShape::Cuboid(s) => {
            if let Some((_, mesh)) = instances
                .cuboid_to_mesh
                .iter()
                .find(|(cuboid, _)| cuboid == s)
            {
                return Some(mesh.clone());
            }

            let (vertices, indices) = s.to_trimesh();
            let mesh = gen_bevy_mesh(&vertices, &indices, true);
            let handle = meshes.add(mesh);
            instances.cuboid_to_mesh.push((s.clone(), handle.clone()));
            return Some(handle);

            // (s.to_trimesh(), true)
        }
        TypedShape::Ball(s) => (s.to_trimesh(NSUB, NSUB / 2), false),
        TypedShape::Cylinder(s) => {
            let (mut vtx, mut idx) = s.to_trimesh(NSUB);
            // Duplicate the basis of the cylinder, to get nice normals.
            let base_id = vtx.len() as u32;

            for i in 0..vtx.len() {
                vtx.push(vtx[i]);
            }

            for idx in &mut idx[NSUB as usize * 2..] {
                idx[0] += base_id;
                idx[1] += base_id;
                idx[2] += base_id;
            }

            ((vtx, idx), false)
        }
        TypedShape::Cone(s) => {
            let (mut vtx, mut idx) = s.to_trimesh(NSUB);
            // Duplicate the basis of the cone, to get nice normals.
            let base_id = vtx.len() as u32;

            for i in 0..vtx.len() - 1 {
                vtx.push(vtx[i]);
            }

            for idx in &mut idx[NSUB as usize..] {
                idx[0] += base_id;
                idx[1] += base_id;
                idx[2] += base_id;
            }

            ((vtx, idx), false)
        }
        TypedShape::Capsule(s) => (s.to_trimesh(NSUB, NSUB / 2), false),
        TypedShape::ConvexPolyhedron(s) => (s.to_trimesh(), true),
        // TypedShape::Compound(s) => s.to_trimesh(),
        TypedShape::HeightField(s) => (s.to_trimesh(), true),
        // TypedShape::Polyline(s) => s.to_trimesh(),
        // TypedShape::Triangle(s) => s.to_trimesh(),
        TypedShape::HalfSpace(s) => {
            let normal = s.normal;
            let extent = 100.0;
            let rot = UnitQuaternion::rotation_between(&Vector::y(), &normal)
                .unwrap_or(UnitQuaternion::identity());
            let vertices = [
                rot * point![extent, 0.0, extent],
                rot * point![extent, 0.0, -extent],
                rot * point![-extent, 0.0, -extent],
                rot * point![-extent, 0.0, extent],
            ];
            let indices = [[0, 1, 2], [0, 2, 3]];
            ((vertices.to_vec(), indices.to_vec()), true)
        }
        TypedShape::TriMesh(s) => ((s.vertices().to_vec(), s.indices().to_vec()), true),
        #[cfg(feature = "voxels")]
        TypedShape::Voxels(s) => (s.to_trimesh(), true),
        _ => todo!(),
    };

    let mesh = gen_bevy_mesh(&vertices, &indices, flat_normals);
    Some(meshes.add(mesh))
}

#[cfg(feature = "dim2")]
fn generate_collision_shape_render_mesh(
    shape: &ColliderShape,
    meshes: &mut Assets<Mesh>,
) -> Option<Handle<Mesh>> {
    const NSUB: u32 = 20;

    let (vertices, indices) = match shape.as_typed_shape() {
        TypedShape::Cuboid(s) => (s.to_polyline(), None),
        TypedShape::Ball(s) => (s.to_polyline(NSUB), None),
        TypedShape::Capsule(s) => (s.to_polyline(NSUB), None),
        // TypedShape::ConvexPolygon(s) => (s.to_polyline(), None),
        // TypedShape::Compound(s) => s.to_polyline(),
        TypedShape::HeightField(s) => return None, // (s.to_polyline(), None),
        // TypedShape::Polyline(s) => s.to_polyline(),
        // TypedShape::Triangle(s) => s.to_polyline(),
        TypedShape::TriMesh(s) => (s.vertices().to_vec(), Some(s.indices().to_vec())),
        _ => todo!(),
    };

    let mesh = gen_bevy_mesh(&vertices, indices);
    Some(meshes.add(mesh))
}

#[cfg(feature = "dim2")]
fn gen_bevy_mesh(vertices: &[Point<Real>], mut indices: Option<Vec<[u32; 3]>>) -> Mesh {
    let mut mesh = Mesh::new(bevy::render::render_resource::PrimitiveTopology::TriangleList);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::from(
            vertices
                .iter()
                .map(|vertex| [vertex.x, vertex.y, 0.0])
                .collect::<Vec<_>>(),
        ),
    );

    if indices.is_none() {
        indices = Some(
            (1..vertices.len() as u32 - 1)
                .map(|i| [0, i, i + 1])
                .collect(),
        );
    }

    mesh.set_indices(Some(Indices::U32(
        indices
            .unwrap()
            .iter()
            .flat_map(|triangle| triangle.iter())
            .cloned()
            .collect(),
    )));

    let normals: Vec<_> = (0..vertices.len()).map(|_| [0.0, 0.0, 1.0]).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, VertexAttributeValues::from(normals));
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        VertexAttributeValues::from(
            (0..vertices.len())
                .map(|_vertex| [0.0, 0.0])
                .collect::<Vec<_>>(),
        ),
    );

    mesh
}

#[cfg(feature = "dim3")]
fn gen_bevy_mesh(vertices: &[Point<Real>], indices: &[[u32; 3]], flat_normals: bool) -> Mesh {
    let mut mesh = Mesh::new(bevy::render::render_resource::PrimitiveTopology::TriangleList);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::from(
            vertices
                .iter()
                .map(|vertex| [vertex.x, vertex.y, vertex.z])
                .collect::<Vec<_>>(),
        ),
    );

    if !flat_normals {
        // Compute vertex normals by averaging the normals
        // of every triangle they appear in.
        // NOTE: This is a bit shonky, but good enough for visualisation.
        let mut normals: Vec<Vec3> = vec![Vec3::ZERO; vertices.len()];
        for triangle in indices.iter() {
            let ab = vertices[triangle[1] as usize] - vertices[triangle[0] as usize];
            let ac = vertices[triangle[2] as usize] - vertices[triangle[0] as usize];
            let normal = ab.cross(&ac);
            // Contribute this normal to each vertex in the triangle.
            for i in 0..3 {
                normals[triangle[i] as usize] += Vec3::new(normal.x, normal.y, normal.z);
            }
        }

        let normals: Vec<[f32; 3]> = normals
            .iter()
            .map(|normal| {
                let normal = normal.normalize();
                [normal.x, normal.y, normal.z]
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, VertexAttributeValues::from(normals));
    }

    // There's nothing particularly meaningful we can do
    // for this one without knowing anything about the overall topology.
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        VertexAttributeValues::from(
            vertices
                .iter()
                .map(|_vertex| [0.0, 0.0])
                .collect::<Vec<_>>(),
        ),
    );
    mesh.set_indices(Some(Indices::U32(
        indices
            .iter()
            .flat_map(|triangle| triangle.iter())
            .cloned()
            .collect(),
    )));

    if flat_normals {
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
    }

    mesh
}
