use std::{collections::BTreeMap, sync::Arc};

use bevy::prelude::ChildBuild;
use bevy::render::mesh::MeshAabb;
use bevy::{
    app::{App, Plugin, PostUpdate, Update},
    asset::{load_internal_asset, Assets, Handle},
    math::{Mat4, Vec3},
    prelude::{
        BuildChildren, Children, Commands, Component, Entity, IntoSystemConfigs, Mesh, Mesh2d,
        Query, Res, ResMut, Shader, Transform, Visibility, With, Without,
    },
    render::{
        view::{NoFrustumCulling, VisibilitySystems},
        RenderApp,
    },
    sprite::{Material2dPlugin, MeshMaterial2d},
};
use blend_pipeline::{BlendType, TrivialBlend};
use material::{BitmapMaterial, GradientMaterial, SwfColorMaterial, SwfMaterial, SwfTransform};
use ruffle_render::transform::Transform as RuffleTransform;

use crate::assets::SwfMovie;
use crate::bundle::FlashAnimation;
use crate::{
    bundle::{ShapeMark, ShapeMarkEntities, SwfGraphicComponent, SwfState},
    plugin::{ShapeDrawType, ShapeMesh},
    swf::display_object::{DisplayObject, TDisplayObject},
};

pub const SWF_COLOR_MATERIAL_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(283691495474896754103765489274589);
pub const GRADIENT_MATERIAL_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(55042096615683885463288330940691701066);
pub const BITMAP_MATERIAL_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(1209708179628049255077713250256144531);

pub mod blend_pipeline;
pub(crate) mod material;
pub(crate) mod node;
pub(crate) mod tessellator;
pub struct FlashRenderPlugin;

impl Plugin for FlashRenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SWF_COLOR_MATERIAL_SHADER_HANDLE,
            "render/shaders/color.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            GRADIENT_MATERIAL_SHADER_HANDLE,
            "render/shaders/gradient.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            BITMAP_MATERIAL_SHADER_HANDLE,
            "render/shaders/bitmap.wgsl",
            Shader::from_wgsl
        );

        app.add_plugins(Material2dPlugin::<GradientMaterial>::default())
            .add_plugins(Material2dPlugin::<SwfColorMaterial>::default())
            .add_plugins(Material2dPlugin::<BitmapMaterial>::default())
            .add_systems(Update, render_swf)
            .add_systems(
                PostUpdate,
                calculate_shape_bounds.in_set(VisibilitySystems::CalculateBounds),
            );

        let Some(_render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
    }
}

#[derive(Component, Default)]
pub struct SwfShapeMesh {
    transform: Mat4,
}

pub fn render_swf(
    mut commands: Commands,
    mut swf_movies: ResMut<Assets<SwfMovie>>,
    mut color_materials: ResMut<Assets<SwfColorMaterial>>,
    mut gradient_materials: ResMut<Assets<GradientMaterial>>,
    mut bitmap_materials: ResMut<Assets<BitmapMaterial>>,
    mut query: Query<(&mut FlashAnimation, Entity)>,
    mut entities_material_query: Query<(
        Entity,
        &mut Transform,
        Option<&MeshMaterial2d<SwfColorMaterial>>,
        Option<&MeshMaterial2d<GradientMaterial>>,
        Option<&MeshMaterial2d<BitmapMaterial>>,
        &mut SwfShapeMesh,
    )>,
    graphic_query: Query<(Entity, &Children), With<SwfGraphicComponent>>,
) {
    for (mut flash_animation, entity) in query.iter_mut() {
        match flash_animation.status {
            SwfState::Loading => {
                continue;
            }
            SwfState::Ready => {
                flash_animation
                    .shape_mark_entities
                    .clear_current_frame_entity();
                if let Some(swf_movie) = swf_movies.get_mut(flash_animation.swf_movie.id()) {
                    let render_list = swf_movie.root_movie_clip.raw_container().render_list();
                    let parent_clip_transform =
                        swf_movie.root_movie_clip.base().transform().clone();
                    let display_objects = swf_movie
                        .root_movie_clip
                        .raw_container_mut()
                        .display_objects_mut();

                    let mut z_index = 0.000;

                    handler_render_list(
                        entity,
                        &graphic_query,
                        &mut commands,
                        &mut color_materials,
                        &mut gradient_materials,
                        &mut bitmap_materials,
                        &mut entities_material_query,
                        &mut flash_animation.shape_mark_entities,
                        render_list,
                        display_objects,
                        &parent_clip_transform,
                        &mut z_index,
                        BlendType::Trivial(TrivialBlend::Normal),
                    );

                    flash_animation
                        .shape_mark_entities
                        .graphic_entities()
                        .iter()
                        .for_each(|(_, entity)| {
                            commands.entity(*entity).insert(Visibility::Hidden);
                        });
                    flash_animation
                        .shape_mark_entities
                        .current_frame_entities()
                        .iter()
                        .for_each(|shape_mark| {
                            let entity = flash_animation
                                .shape_mark_entities
                                .entity(shape_mark)
                                .unwrap();
                            commands.entity(*entity).insert(Visibility::Inherited);
                        });
                    flash_animation.status = SwfState::Loading;
                }
            }
        }
    }
}

pub fn handler_render_list(
    parent_entity: Entity,
    graphic_children_entities: &Query<
        '_,
        '_,
        (bevy::prelude::Entity, &Children),
        With<SwfGraphicComponent>,
    >,
    commands: &mut Commands,
    color_materials: &mut ResMut<Assets<SwfColorMaterial>>,
    gradient_materials: &mut ResMut<Assets<GradientMaterial>>,
    bitmap_materials: &mut ResMut<Assets<BitmapMaterial>>,
    entities_material_query: &mut Query<
        '_,
        '_,
        (
            Entity,
            &mut Transform,
            Option<&MeshMaterial2d<SwfColorMaterial>>,
            Option<&MeshMaterial2d<GradientMaterial>>,
            Option<&MeshMaterial2d<BitmapMaterial>>,
            &mut SwfShapeMesh,
        ),
    >,
    shape_mark_entities: &mut ShapeMarkEntities,
    render_list: Arc<Vec<u128>>,
    display_objects: &mut BTreeMap<u128, DisplayObject>,
    parent_clip_transform: &RuffleTransform,
    z_index: &mut f32,
    blend_type: BlendType,
) {
    for display_object in render_list.iter() {
        if let Some(display_object) = display_objects.get_mut(display_object) {
            match display_object {
                DisplayObject::Graphic(graphic) => {
                    let current_transform = graphic.base().transform();
                    let swf_transform: SwfTransform = RuffleTransform {
                        matrix: parent_clip_transform.matrix * current_transform.matrix,
                        color_transform: parent_clip_transform.color_transform
                            * current_transform.color_transform,
                    }
                    .into();
                    // 记录当前帧生成的graphic实体
                    let mut shape_mark = ShapeMark {
                        graphic_ref_count: 1,
                        depth: graphic.depth(),
                        id: graphic.character_id(),
                    };
                    while let Some(_) = shape_mark_entities
                        .current_frame_entities()
                        .iter()
                        .find(|&x| *x == shape_mark)
                    {
                        shape_mark.graphic_ref_count += 1;
                    }
                    *z_index += graphic.depth() as f32 / 100.0;
                    if let Some(&existing_entity) = shape_mark_entities.entity(&shape_mark) {
                        // 存在缓存实体
                        if let Some((_, graphic_children)) = graphic_children_entities
                            .iter()
                            .find(|(entity, _)| *entity == existing_entity)
                        {
                            graphic_children.iter().for_each(|child| {
                                for (
                                    material_entity,
                                    mut transform,
                                    swf_color_material_handle,
                                    swf_gradient_material_handle,
                                    swf_bitmap_material_handle,
                                    mut swf_shape_mesh,
                                ) in entities_material_query.iter_mut()
                                {
                                    if material_entity == *child {
                                        *z_index += 0.001;
                                        transform.translation.z = *z_index;
                                        if let Some(handle) = swf_color_material_handle {
                                            update_swf_material(
                                                (handle, swf_shape_mesh.as_mut()),
                                                color_materials,
                                                swf_transform.clone(),
                                            );
                                            break;
                                        }
                                        if let Some(handle) = swf_gradient_material_handle {
                                            update_swf_material(
                                                (handle, swf_shape_mesh.as_mut()),
                                                gradient_materials,
                                                swf_transform.clone(),
                                            );
                                            break;
                                        }
                                        if let Some(handle) = swf_bitmap_material_handle {
                                            update_swf_material(
                                                (handle, swf_shape_mesh.as_mut()),
                                                bitmap_materials,
                                                swf_transform.clone(),
                                            );
                                            break;
                                        }
                                    }
                                }
                            });
                        }
                    } else {
                        // 不存在缓存实体
                        let graphic_entity = commands
                            .spawn((
                                SwfGraphicComponent,
                                Transform::default(),
                                Visibility::default(),
                            ))
                            .id();
                        commands.entity(parent_entity).add_child(graphic_entity);
                        shape_mark_entities.add_entities_pool(shape_mark, graphic_entity);
                        graphic.shape_mesh().iter_mut().for_each(|shape| {
                            *z_index += 0.001;
                            let transform =
                                Transform::from_translation(Vec3::new(0.0, 0.0, *z_index));
                            match &shape.draw_type {
                                ShapeDrawType::Color(swf_color_material) => {
                                    spawn_mesh(
                                        commands,
                                        swf_color_material.clone(),
                                        color_materials,
                                        swf_transform.clone(),
                                        graphic_entity,
                                        transform,
                                        shape,
                                    );
                                }
                                ShapeDrawType::Gradient(gradient_material) => {
                                    spawn_mesh(
                                        commands,
                                        gradient_material.clone(),
                                        gradient_materials,
                                        swf_transform.clone(),
                                        graphic_entity,
                                        transform,
                                        shape,
                                    );
                                }
                                ShapeDrawType::Bitmap(bitmap_material) => {
                                    spawn_mesh(
                                        commands,
                                        bitmap_material.clone(),
                                        bitmap_materials,
                                        swf_transform.clone(),
                                        graphic_entity,
                                        transform,
                                        shape,
                                    );
                                }
                            }
                        });
                    }
                    shape_mark_entities.record_current_frame_entity(shape_mark);
                }
                DisplayObject::MovieClip(movie_clip) => {
                    let current_transform = RuffleTransform {
                        matrix: parent_clip_transform.matrix * movie_clip.base().transform().matrix,
                        color_transform: parent_clip_transform.color_transform
                            * movie_clip.base().transform().color_transform,
                    };
                    let blend_type = BlendType::from(movie_clip.blend_mode());

                    handler_render_list(
                        parent_entity,
                        graphic_children_entities,
                        commands,
                        color_materials,
                        gradient_materials,
                        bitmap_materials,
                        entities_material_query,
                        shape_mark_entities,
                        movie_clip.raw_container().render_list(),
                        movie_clip.raw_container_mut().display_objects_mut(),
                        &current_transform,
                        z_index,
                        blend_type,
                    );
                }
            }
        }
    }
}

#[inline]
fn update_swf_material<T: SwfMaterial>(
    exists_material: (&Handle<T>, &mut SwfShapeMesh),
    swf_materials: &mut ResMut<Assets<T>>,
    swf_transform: SwfTransform,
) {
    // 当缓存某实体后该实体在该系统尚未运行完成时会查询不到对应的材质，此时重新生成材质。
    if let Some(swf_material) = swf_materials.get_mut(exists_material.0) {
        let swf_shape_mesh = exists_material.1;
        swf_shape_mesh.transform = swf_transform.world_transform;
        swf_material.update_swf_material(swf_transform);
    }
}

#[inline]
fn spawn_mesh<T: SwfMaterial>(
    commands: &mut Commands,
    mut swf_material: T,
    swf_materials: &mut ResMut<Assets<T>>,
    swf_transform: SwfTransform,
    parent_entity: Entity,
    transform: Transform,
    shape: &ShapeMesh,
) {
    swf_material.update_swf_material(swf_transform);
    let aabb_transform = swf_material.world_transform();
    commands.entity(parent_entity).with_children(|parent| {
        parent.spawn((
            Mesh2d(shape.mesh.clone()),
            MeshMaterial2d(swf_materials.add(swf_material)),
            transform,
            SwfShapeMesh {
                transform: aabb_transform,
            },
        ));
    });
}

pub fn calculate_shape_bounds(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    meshes_without_aabb: Query<(Entity, &Mesh2d, &SwfShapeMesh), Without<NoFrustumCulling>>,
) {
    meshes_without_aabb
        .iter()
        .for_each(|(entity, mesh_handle, swf_shape_mesh)| {
            if let Some(mesh) = meshes.get(&mesh_handle.0) {
                if let Some(mut aabb) = mesh.compute_aabb() {
                    let swf_transform = Mat4::from_cols_array_2d(&[
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, -1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ]) * swf_shape_mesh.transform;
                    aabb.center = swf_transform.transform_point3a(aabb.center);
                    commands.entity(entity).try_insert(aabb);
                }
            }
        });
}
