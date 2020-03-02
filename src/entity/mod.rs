//! Entity management and structures

mod info;
mod sys;

pub use self::info::*;

use crate::ecs;
use crate::prelude::*;
use crate::render::animated_model;
use crate::server::assets;
use crate::server::common;
use crate::server::entity::*;

/// Registers components required by the client
pub fn register_components(c: &mut ecs::Container) {
    c.register_component::<Model>();
    c.register_component::<ModelTexture>();
    c.register_component::<StaticModel>();
    c.register_component::<AnimatedModel>();
    c.register_component::<Delta>();
    c.register_component::<CursorPosition>();
    c.register_component::<AnimationMovementSpeed>();
    c.register_component::<Icon>();
    c.register_component::<Color>();
    c.register_component::<FadeOverLife>();
    c.register_component::<crate::audio::AudioController>();
    c.register_component::<animated_model::InfoTick>();
    c.register_component::<Highlighted>();
    c.register_component::<crate::instance::scripting::LuaEntityRef>();
    c.register_component::<AttachedTo>();
    c.register_component::<ClientBooked>();
}

/// Registers systems required by the client
pub fn register_systems(sys: &mut ecs::Systems) {
    sys.add(sys::animate_door);
    sys.add(sys::animate_walking);
    sys.add(sys::animate_movement_speed);
    sys.add(sys::fade_lifetime);
    sys.add(sys::tick_emotes);
    sys.add(sys::remove_attachments);
    sys.add(sys::remove_attachments_room);
}

/// Registers systems required by the client that will be run
/// every frame.
///
/// The systems here should be pretty limited
pub fn register_frame_systems(sys: &mut ecs::Systems) {
    sys.add(crate::server::entity::pathfind::tick_pathfinder);
    sys.add(sys::lerp_target_pos);
    sys.add(sys::lerp_target_rot);
    sys.add(crate::server::entity::follow_sys);
    sys.add(crate::server::entity::follow_rot);
    sys.add(sys::tick_animations);
}

/// Handles creating entities for the server
pub enum ClientEntityCreator {}

impl EntityCreator for ClientEntityCreator {
    type ScriptTypes = crate::instance::scripting::Types;
    fn static_model(
        c: &mut ecs::Container,
        model: assets::ResourceKey<'_>,
        texture: Option<assets::ResourceKey<'_>>,
    ) -> ecs::Entity {
        let e = ServerEntityCreator::static_model(
            c,
            model.borrow(),
            texture.as_ref().map(|v| v.borrow()),
        );
        c.add_component(
            e,
            Model {
                name: model.into_owned(),
            },
        );
        c.add_component(e, StaticModel);
        if let Some(tex) = texture {
            c.add_component(
                e,
                ModelTexture {
                    name: tex.into_owned(),
                },
            );
        }
        e
    }
    fn animated_model(
        c: &mut ecs::Container,
        model: assets::ResourceKey<'_>,
        texture: Option<assets::ResourceKey<'_>>,
        animation_set: common::AnimationSet,
        animation: &str,
    ) -> ecs::Entity {
        let e = ServerEntityCreator::animated_model(
            c,
            model.borrow(),
            texture.as_ref().map(|v| v.borrow()),
            animation_set.clone(),
            animation,
        );
        c.add_component(
            e,
            Model {
                name: model.into_owned(),
            },
        );
        c.add_component(e, AnimatedModel::new(animation_set, animation));
        if let Some(tex) = texture {
            c.add_component(
                e,
                ModelTexture {
                    name: tex.into_owned(),
                },
            );
        }
        e
    }
}

/// Delta time between the current frame and the last frame
pub struct Delta(pub f64);
component!(Delta => const World);

/// The position of the cursor in level coordinates
pub struct CursorPosition {
    /// The position along the x axis
    pub x: f32,
    /// The position along the y axis
    pub y: f32,
}
component!(CursorPosition => const World);

/// A model to render
pub struct Model {
    /// The name of the model to use
    pub name: assets::ResourceKey<'static>,
}
component!(Model => Vec);

/// A model to render
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelTexture {
    /// The name of the model to use
    pub name: assets::ResourceKey<'static>,
}
component!(ModelTexture => Vec);

/// Marks a model as static (not animated)
#[derive(Default)]
pub struct StaticModel;
component!(StaticModel => Marker);

/// Ties the animation speed to the movement speed
/// of the entity.
pub struct AnimationMovementSpeed {
    /// The animation speed modifier
    pub modifier: f64,
}
component!(AnimationMovementSpeed => Map);

/// Marks a model as animated
pub struct AnimatedModel {
    /// The name of the animations to play.
    ///
    /// Animations will keep the last one until another one is queued
    pub animation_queue: Vec<String>,
    /// The offset into the animation the model is
    /// currently at.
    pub time: f64,
    /// The speed of the animation
    pub speed: f64,
    /// The collection of animations to play from
    pub animation_set: common::AnimationSet,
}
component!(AnimatedModel => Vec);

impl AnimatedModel {
    /// Creates an animated model component set to play the named
    /// animation.
    pub fn new<S: Into<String>>(
        animation_set: common::AnimationSet,
        animation: S,
    ) -> AnimatedModel {
        AnimatedModel {
            animation_set,
            animation_queue: vec![animation.into()],
            time: 0.0,
            speed: 1.0,
        }
    }

    /// Changes the animation currently playing
    pub fn set_animation<S: Into<String> + PartialEq<String>>(&mut self, name: S) {
        if self.animation_queue.len() != 1
            || self.animation_queue.first().map_or(true, |v| name != *v)
        {
            let name = name.into();
            self.time = 0.0;
            self.speed = 1.0;
            self.animation_queue.clear();
            self.animation_queue.push(name);
        }
    }

    /// Queues the animation to play next
    pub fn queue_animation<S: Into<String> + PartialEq<String>>(&mut self, name: S) {
        if self.animation_queue.last().map_or(true, |v| name != *v) {
            self.animation_queue.push(name.into());
        }
    }

    /// Returns the currently playing animation
    pub fn current_animation(&self) -> Option<&str> {
        self.animation_queue.first().map(|v| &v[..])
    }
}

/// Attaches this entity to another entity.
///
/// The other entity must be animated for this to function
/// correctly.
#[derive(Debug)]
pub struct AttachedTo {
    /// The entity this is attached to
    pub target: ecs::Entity,
    /// The bone this is attached to
    pub bone: String,
    /// The offset matrix to apply to the attachment to
    /// align it
    pub offset: cgmath::Matrix4<f32>,
}
component!(AttachedTo => Vec);

/// In world icon rendered as a billboard that
/// always faces the camera
pub struct Icon {
    /// The texture to render
    pub texture: assets::ResourceKey<'static>,
    /// The size of the icon
    pub size: (f32, f32),
}
component!(Icon => Map);

/// Tints a model/icon
pub struct Color {
    /// Color of this entity
    pub color: (u8, u8, u8, u8),
}
component!(Color => Map);

/// Cause the entity's alpha channel to fade based
/// on the entity's remaining life
#[derive(Default)]
pub struct FadeOverLife;
component!(FadeOverLife => Marker);

/// Highlights the entity with the given color
pub struct Highlighted {
    /// The hightlight color
    pub color: (u8, u8, u8),
}
component!(Highlighted => Map);

/// Client-side marker on whether a room is booked or not
#[derive(Default)]
pub struct ClientBooked;
component!(ClientBooked => Marker);
