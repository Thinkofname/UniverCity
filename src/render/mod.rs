//! Contains everything to do with rendering the game.
//! OpenGL (or other APIs) should only be used in this module
//! and must not spread into other modules.

#[allow(clippy::enum_variant_names)]
#[allow(clippy::new_without_default)]
#[allow(clippy::too_many_arguments)]
pub mod gl;
mod atlas;
mod model;
mod terrain;
pub(crate) mod image;
mod static_model;
pub(crate) mod animated_model;
pub(crate) mod ui;
mod icons;
#[macro_use]
mod pipeline;

use crate::keybinds;
use crate::entity;
use crate::server::level;
use sdl2;
use crate::server::assets;
use crate::util::*;
use cgmath::{self, Matrix4};
use cgmath::prelude::*;
use crate::ecs;
use crate::math::Frustum;
use crate::server::player;
use crate::prelude::*;

use ::model as exmodel;
use self::pipeline::Pipeline;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::borrow::Borrow;
use sdl2::mouse::Cursor;

const ATLAS_SIZE: i32 = 2048;
/// The texture unit the global texture atlas will be bound to.
const GLOBAL_TEXTURE_LOCATION: u32 = 5;
/// The texture unit the shadow map will be bound to.
pub const SHADOW_MAP_LOCATION: u32 = 4;

const MAX_ZOOM_OUT: f32 = 0.2;
const MAX_ZOOM_IN: f32 = 2.5;

type GlobalTextureMap = FNVMap<assets::ResourceKey<'static>, (i32, atlas::Rect)>;
type LoadingTexture = Vec<(image::ImageFuture, i32, atlas::Rect)>;

/// Controls rendering the game, should only exist once per a game session
/// but it may be possible to recreate it once the previous instance is
/// gone to restart the renderer.
pub struct Renderer {
    video_system: sdl2::VideoSubsystem,
    state: RenderState,
    pipeline: Pipeline<PassFlag>,
    ui_renderer: ui::Renderer,
    _context: sdl2::video::GLContext,
}

impl Deref for Renderer {
    type Target = RenderState;
    fn deref(&self) -> &RenderState {
        &self.state
    }
}

impl DerefMut for Renderer {
    fn deref_mut(&mut self) -> &mut RenderState {
        &mut self.state
    }
}

/// Render state information
pub struct RenderState {
    /// The logger used by the renderer
    pub log: Logger,
    asset_manager: assets::AssetManager,
    config: Rc<Config>,

    camera: Camera,
    shadow_rotation: cgmath::Deg<f32>,
    // Used for effects
    time: f64,
    /// Whether to use the paused effect
    pub paused: bool,

    /// The width of the screen
    pub width: u32,
    /// The height of the screen
    pub height: u32,

    terrain: Option<terrain::Terrain>,

    /// The cursor is a box that follows the players cursor in the world.
    pub cursor: model::Model<cgmath::Vector3<f32>>,
    /// Flags whether the cursor model should be rendered or not
    pub cursor_visible: bool,
    selection: Option<Selection>,

    mouse_pos: (i32, i32),
    mouse_sprite: ResourceKey<'static>,
    mouse_cursor: Option<Cursor>,

    focused_region: Option<Bound>,

    // Text input support
    is_text_input: bool,
    text_input_region: Option<(i32, i32, i32, i32)>,

    // Textures that can be used by anything
    global_atlas: GlobalAtlas,

    static_info: static_model::Info,
    pub(crate) animated_info: animated_model::Info,
    icons: icons::Icons,
}

struct Camera {
    x: f32,
    y: f32,
    target: Option<(f32, f32, f64)>,
    /// Sets the rotation of the camera in degrees.
    rotation: cgmath::Deg<f32>,
    /// Controls the zoom level of the camera
    zoom: f32,
    movement: [bool; 4],
}

struct GlobalAtlas {
    textures: GlobalTextureMap,
    texture: gl::Texture,
    atlases: Vec<atlas::TextureAtlas>,
    loading_textures: LoadingTexture,
}

struct Selection {
    selection_model: model::Model<terrain::GLVertex>,
    start: Location,
    current: Location,
    cycle: f64,
}

bitflags! {
    struct PassFlag: u8 {
        const NO_ICONS   = 0b0000_0001;
        const NO_CURSOR  = 0b0000_0010;
        const NO_TERRAIN = 0b0000_0100;
        const HIGHLIGHTS = 0b0000_1000;
    }
}

impl Renderer {
    /// Creates a new renderer instance. This will create a OpenGL context and bind it
    /// to the current thread. Only one renderer instance should exist at any given time
    /// due to the dependance on OpenGL which uses thread locals.
    pub fn new(log: &Logger, window: &sdl2::video::Window, asset_manager: assets::AssetManager, config: Rc<Config>) -> Option<Renderer> {
        let log = log.new(o!(
            "gl_version" => "3.3 Core"
        ));
        let video = window.subsystem();
        let gl_context = match window.gl_create_context() {
            Ok(val) => val,
            Err(err) => {
                error!(log, "Failed to create GL3.3 context"; "error" => %err);
                return None;
            }
        };
        window.gl_make_current(&gl_context).expect("Could not set current context.");
        gl::load_with(|s| video.gl_get_proc_address(s) as *const _);
        info!(log, "Using renderer: OpenGL 3.3");

        gl::enable(gl::Flag::DepthTest);
        gl::enable(gl::Flag::CullFace);
        gl::front_face(gl::Face::ClockWise);
        gl::cull_face(gl::CullFace::Back);
        gl::depth_func(gl::Func::GreaterOrEqual);
        gl::clear_depth(0.0);

        let texture = gl::Texture::new();
        texture.bind(gl::TextureTarget::Texture2DArray);
        texture.image_3d(
            gl::TextureTarget::Texture2DArray, 0,
            ATLAS_SIZE as u32, ATLAS_SIZE as u32, 1,
            gl::TextureFormat::Srgba8, gl::TextureFormat::Rgba,
            gl::Type::UnsignedByte,
            None
        );
        texture.set_parameter::<gl::TextureMinFilter>(gl::TextureTarget::Texture2DArray, gl::TextureFilter::Nearest);
        texture.set_parameter::<gl::TextureMagFilter>(gl::TextureTarget::Texture2DArray, gl::TextureFilter::Nearest);
        texture.set_parameter::<gl::TextureWrapS>(gl::TextureTarget::Texture2DArray, gl::TextureWrap::ClampToEdge);
        texture.set_parameter::<gl::TextureWrapT>(gl::TextureTarget::Texture2DArray, gl::TextureWrap::ClampToEdge);
        texture.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2DArray, 0);
        texture.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2DArray, 0);

        let mut pipeline = Self::build_pipeline(&log, &config, &asset_manager);

        let static_info = static_model::Info::new(&log, &mut pipeline.context(), &asset_manager);
        let animated_info = animated_model::Info::new(&log, &asset_manager, &mut pipeline.context());
        let icons = icons::Icons::new(&log, &mut pipeline.context());

        let ui_renderer = ui::Renderer::new(&log, &asset_manager, &mut pipeline.context());

        let mut renderer = Renderer {
            ui_renderer,
            video_system: video.clone(),
            state: RenderState {
                log,
                asset_manager,
                config,
                time: 0.0,
                paused: false,

                cursor: model::Model::new(
                    &mut pipeline.context(),
                    "cursor",
                    vec![
                        model::Attribute {name: "attrib_position", count: 3, ty: gl::Type::Float, offset: 0, int: false},
                    ],
                    vec![
                        cgmath::Vector3 {x: 0.0, y: 0.0, z: 0.0},
                        cgmath::Vector3 {x: 0.0, y: 0.0, z: 1.0},
                        cgmath::Vector3 {x: 1.0, y: 0.0, z: 0.0},
                        cgmath::Vector3 {x: 1.0, y: 0.0, z: 0.0},
                        cgmath::Vector3 {x: 0.0, y: 0.0, z: 1.0},
                        cgmath::Vector3 {x: 1.0, y: 0.0, z: 1.0}
                    ],
                ),
                cursor_visible: false,

                terrain: None,
                focused_region: None,

                camera: Camera {
                    x: 0.0,
                    y: 0.0,
                    target: None,
                    rotation: cgmath::Deg(0.0),
                    zoom: 0.8,
                    movement: [false; 4],
                },
                shadow_rotation: cgmath::Deg(-90.0),

                width: 800,
                height: 480,

                selection: None,

                mouse_pos: (0, 0),
                mouse_sprite: ResourceKey::new("", ""),
                mouse_cursor: None,

                is_text_input: false,
                text_input_region: None,

                global_atlas: GlobalAtlas {
                    texture,
                    textures: FNVMap::default(),
                    atlases: vec![],
                    loading_textures: vec![],
                },

                static_info,
                animated_info,
                icons,
            },
            _context: gl_context,
            pipeline,
        };
        renderer.set_mouse_sprite(ResourceKey::new("base", "ui/cursor/normal"));
        Some(renderer)
    }

    /// Causes the renderer to rebuild the whole pipeline with the current settings
    pub fn rebuild_pipeline(&mut self) {
        self.pipeline.clear();
        self.pipeline = Self::build_pipeline(&self.log, &self.config, &self.asset_manager);
    }

    fn build_pipeline(log: &Logger, config: &Config, asset_manager: &AssetManager) -> Pipeline<PassFlag> {
        use cgmath::Vector3;

        // Flags
        let shadow_size = config.render_shadow_res.get();
        let shadows_enabled = shadow_size > 0;
        let ssao_level = config.render_ssao.get();
        let ssao_enabled = ssao_level > 0;
        let fxaa_enabled = config.render_fxaa.get();
        let render_scale: f32 = config.render_scale.get();
        let render_scaling: bool = render_scale < 1.0;

        let selection_valid = config.placement_valid_colour.get();
        let selection_valid = format!(
            "vec3({}, {}, {})",
            f32::from(selection_valid.0) / 255.0,
            f32::from(selection_valid.1) / 255.0,
            f32::from(selection_valid.2) / 255.0,
        );
        let selection_invalid = config.placement_invalid_colour.get();
        let selection_invalid = format!(
            "vec3({}, {}, {})",
            f32::from(selection_invalid.0) / 255.0,
            f32::from(selection_invalid.1) / 255.0,
            f32::from(selection_invalid.2) / 255.0,
        );

        #[allow(clippy::unreadable_literal)]
        const KERNEL_SAMPLE: &[cgmath::Vector3<f32>] = &[
            Vector3{x: -0.062373966, y: 0.10192162, z: 0.14568073},
            Vector3{x: 0.3113865, y: -0.57439685, z: 0.39331475},
            Vector3{x: 0.40664312, y: 0.19381301, z: 0.4290815},
            Vector3{x: -0.28126782, y: 0.3163654, z: 0.058499716},
            Vector3{x: -0.07170813, y: 0.45036793, z: 0.52463275},
            Vector3{x: 0.063700885, y: 0.056648985, z: 0.06263993},
            Vector3{x: 0.1720627, y: -0.6995218, z: 0.69317275},
            Vector3{x: 0.43557528, y: 0.13584973, z: 0.6312793},
            Vector3{x: 0.03560861, y: 0.022413144, z: 0.44989896},
            Vector3{x: 0.5447034, y: 0.039564166, z: 0.11858774},
            Vector3{x: 0.5864158, y: -0.14208466, z: 0.34905744},
            Vector3{x: 0.13498332, y: -0.052775092, z: 0.0584181},
            Vector3{x: -0.04370347, y: -0.8014861, z: 0.39388356},
            Vector3{x: 0.5435809, y: 0.6014337, z: 0.45571947},
            Vector3{x: -0.017261483, y: -0.31715277, z: 0.28491244},
            Vector3{x: 0.28214985, y: -0.054105192, z: 0.33074465},
            Vector3{x: 0.51650006, y: -0.10689126, z: 0.60269225},
            Vector3{x: -0.68348897, y: -0.23492643, z: 0.61831063},
            Vector3{x: 0.0005352287, y: -0.035418484, z: 0.01681905},
            Vector3{x: -0.29614896, y: -0.8263739, z: 0.29955795},
            Vector3{x: 0.15246555, y: -0.19299337, z: 0.11754207},
            Vector3{x: 0.036504924, y: 0.049158994, z: 0.07902589},
            Vector3{x: 0.077379376, y: 0.0044365656, z: 0.15595515},
            Vector3{x: 0.07508131, y: 0.16889392, z: 0.18840827},
            Vector3{x: 0.7593624, y: -0.30594045, z: 0.066411026},
            Vector3{x: 0.13855128, y: 0.21852091, z: 0.31970432},
            Vector3{x: -0.25437066, y: 0.44493994, z: 0.18426113},
            Vector3{x: 0.042830702, y: 0.1290134, z: 0.08635309},
            Vector3{x: -0.6790049, y: -0.5031178, z: 0.008023582},
            Vector3{x: 0.007144741, y: -0.90833974, z: 0.011879381},
            Vector3{x: 0.6525548, y: 0.6258723, z: 0.37451333},
            Vector3{x: -0.4938841, y: 0.5239768, z: 0.5979795},
            Vector3{x: -0.006661688, y: 0.044782795, z: 0.056101732},
            Vector3{x: 0.0080028195, y: -0.14824994, z: 0.16872679},
            Vector3{x: 0.49335507, y: 0.47488365, z: 0.061385423},
            Vector3{x: 0.058942337, y: -0.07240473, z: 0.032073382},
            Vector3{x: -0.30052447, y: -0.24517778, z: 0.018952174},
            Vector3{x: 0.015380497, y: 0.046154376, z: 0.021703264},
            Vector3{x: -0.6107009, y: -0.502106, z: 0.06270276},
            Vector3{x: 0.3474558, y: 0.10106672, z: 0.10467811},
            Vector3{x: -0.7580435, y: -0.4087925, z: 0.2990682},
            Vector3{x: 0.33691484, y: 0.09465869, z: 0.31044757},
            Vector3{x: 0.2032441, y: -0.41798624, z: 0.26093316},
            Vector3{x: -0.35458136, y: -0.35704505, z: 0.13314952},
            Vector3{x: 0.36712593, y: 0.11435944, z: 0.75961924},
            Vector3{x: -0.08748649, y: -0.4340488, z: 0.21883406},
            Vector3{x: -0.35036975, y: -0.44294822, z: 0.38801843},
            Vector3{x: 0.16525006, y: -0.4293002, z: 0.38747865},
            Vector3{x: 0.04050224, y: -0.018665602, z: 0.09477742},
            Vector3{x: 0.331764, y: 0.18187577, z: 0.039461646},
            Vector3{x: 0.27246836, y: -0.31345794, z: 0.1313441},
            Vector3{x: -0.018983915, y: -0.034603603, z: 0.043682262},
            Vector3{x: 0.620421, y: 0.5948607, z: 0.50955445},
            Vector3{x: 0.40889615, y: -0.3072589, z: 0.20670788},
            Vector3{x: -0.5986124, y: -0.092387445, z: 0.42859203},
            Vector3{x: -0.23566812, y: -0.30504203, z: 0.523666},
            Vector3{x: -0.7431781, y: 0.6464084, z: 0.151515},
            Vector3{x: 0.13415746, y: -0.10305414, z: 0.121167466},
            Vector3{x: -0.19648616, y: -0.24216011, z: 0.2817499},
            Vector3{x: -0.544367, y: -0.15035674, z: 0.20823056},
            Vector3{x: 0.011894039, y: 0.3929868, z: 0.34688452},
            Vector3{x: 0.64825016, y: 0.0694976, z: 0.2940839},
            Vector3{x: 0.17234638, y: 0.51166487, z: 0.7982096},
            Vector3{x: -0.17149533, y: -0.14325161, z: 0.06901508},
        ];
        #[allow(clippy::unreadable_literal)]
        const NOISE: &[cgmath::Vector3<f32>] = &[
            Vector3{x: 0.6236799, y: 0.28953505, z: 0.0},
            Vector3{x: -0.04945755, y: -0.13388228, z: 0.0},
            Vector3{x: -0.9266672, y: -0.11407256, z: 0.0},
            Vector3{x: 0.64765406, y: -0.3466444, z: 0.0},
            Vector3{x: 0.13071871, y: -0.949934, z: 0.0},
            Vector3{x: 0.9816792, y: 0.9918339, z: 0.0},
            Vector3{x: -0.16705489, y: -0.13878465, z: 0.0},
            Vector3{x: 0.67325187, y: 0.79079103, z: 0.0},
            Vector3{x: 0.97574115, y: 0.98447084, z: 0.0},
            Vector3{x: -0.7927344, y: 0.04460287, z: 0.0},
            Vector3{x: 0.9778807, y: -0.3611865, z: 0.0},
            Vector3{x: 0.15127349, y: -0.44163084, z: 0.0},
            Vector3{x: -0.52516866, y: 0.040046692, z: 0.0},
            Vector3{x: 0.5218489, y: -0.96062183, z: 0.0},
            Vector3{x: -0.5614555, y: -0.19064069, z: 0.0},
            Vector3{x: -0.51988673, y: 0.44126797, z: 0.0},
        ];

        let mut ssao_kernel = Vec::with_capacity(ssao_level as usize);
        for (i, sample) in KERNEL_SAMPLE.iter().cloned().enumerate().take(ssao_level as usize) {
            let scale = i as f32 / (ssao_level as f32 * 2.0);
            let scale = 0.1 + 0.9 * (scale * scale);
            ssao_kernel.push(sample * scale);
        }

        let noise_tex = gl::Texture::new();
        noise_tex.bind(gl::TextureTarget::Texture2D);
        noise_tex.image_2d_any(gl::TextureTarget::Texture2D, 0, 4, 4, gl::TextureFormat::Rgb16F, gl::TextureFormat::Rgb, gl::Type::Float, Some(NOISE));
        noise_tex.set_parameter::<gl::TextureMinFilter>(gl::TextureTarget::Texture2D, gl::TextureFilter::Nearest);
        noise_tex.set_parameter::<gl::TextureMagFilter>(gl::TextureTarget::Texture2D, gl::TextureFilter::Nearest);
        noise_tex.set_parameter::<gl::TextureWrapS>(gl::TextureTarget::Texture2D, gl::TextureWrap::Repeat);
        noise_tex.set_parameter::<gl::TextureWrapT>(gl::TextureTarget::Texture2D, gl::TextureWrap::Repeat);

        let fullscreen_attribs = &[
            ("attrib_position", 0)
        ];

        Pipeline::<PassFlag>::new(log, asset_manager.clone(), render_scale)
            // UI Shaders
            .program("ui/clip", |_, p| p
                .vertex("ui/clip_vert")
                .fragment("ui/clip_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                ]))
            .program("ui/box", |_, p| p
                .vertex("ui/box_vert")
                .fragment("ui/box_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_color", 1),
                ]))
            .program("ui/box_shadow", |_, p| p
                .vertex("ui/box_shadow_vert")
                .fragment("ui/box_shadow_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_color", 1),
                ]))
            .program("ui/box_shadow_inner", |_, p| p
                .vertex("ui/box_shadow_vert")
                .fragment("ui/box_shadow_inner_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_color", 1),
                ]))
            .program("ui/image", |_, p| p
                .vertex("ui/image_vert")
                .fragment("ui/image_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_texture_info", 1),
                    ("attrib_atlas", 2),
                    ("attrib_uv", 3),
                    ("attrib_color", 4),
                ]))
            .program("ui/text", |_, p| p
                .vertex("ui/text_vert")
                .fragment("ui/text_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_texture_info", 1),
                    ("attrib_atlas", 2),
                    ("attrib_uv", 3),
                ]))
            .program("ui/text_shadow", |_, p| p
                .vertex("ui/text_vert")
                .fragment("ui/text_shadow_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_texture_info", 1),
                    ("attrib_atlas", 2),
                    ("attrib_uv", 3),
                ]))
            .program("ui/border_image", |_, p| p
                .vertex("ui/border_image_vert")
                .fragment("ui/border_image_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_texture_info", 1),
                    ("attrib_atlas", 2),
                    ("attrib_uv", 3),
                    ("attrib_color", 4),
                ]))
            .program("ui/border_solid", |_, p| p
                .vertex("ui/border_solid_vert")
                .fragment("ui/border_solid_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_rel_position", 1),
                    ("attrib_color", 2),
                    ("attrib_style", 3),
                ]))
            // Game shaders
            .program("merge", |_, p| p
                .vertex("fullscreen_vert")
                .fragment("merge")
                .when(ssao_enabled, |p| p
                    .fragment_defines(vec!["ssao_enabled"])
                )
                .when(shadows_enabled, |p| p
                    .fragment_defines(vec!["shadows_enabled"])
                )
                .attribute_binds(fullscreen_attribs))
            .program("pause_effect", |_, p| p
                .vertex("fullscreen_vert")
                .fragment("pause_effect")
                .attribute_binds(fullscreen_attribs))
            .program("focused_effect", |_, p| p
                .vertex("fullscreen_vert")
                .fragment("focused_effect")
                .attribute_binds(fullscreen_attribs))
            .program("scale", |_, p| p
                .enabled(render_scaling)
                .vertex("fullscreen_vert")
                .fragment("scale")
                .fragment_defines(vec![format!("render_scale {}", render_scale)])
                .attribute_binds(fullscreen_attribs))
            .program("fxaa", |_, p| p
                .enabled(fxaa_enabled)
                .vertex("fullscreen_vert")
                .fragment("fxaa")
                .attribute_binds(fullscreen_attribs))
            .program("ssao", |_, p| p
                .enabled(ssao_enabled)
                .vertex("fullscreen_vert")
                .fragment("ssao")
                .fragment_defines(vec![format!("kernel_size {}", ssao_level)])
                .attribute_binds(fullscreen_attribs))
            .program("blur_ssao_horz", |_, p| p
                .enabled(ssao_enabled)
                .vertex("fullscreen_vert")
                .fragment("blur_ssao")
                .fragment_defines(vec!["HORZ"])
                .attribute_binds(fullscreen_attribs))
            .program("blur_ssao_vert", |_, p| p
                .enabled(ssao_enabled)
                .vertex("fullscreen_vert")
                .fragment("blur_ssao")
                .fragment_defines(vec!["VERT"])
                .attribute_binds(fullscreen_attribs))
            .program("blur_color_horz", |_, p| p
                .vertex("fullscreen_vert")
                .fragment("blur_color")
                .fragment_defines(vec!["HORZ"])
                .attribute_binds(fullscreen_attribs))
            .program("blur_color_vert", |_, p| p
                .vertex("fullscreen_vert")
                .fragment("blur_color")
                .fragment_defines(vec!["VERT"])
                .attribute_binds(fullscreen_attribs))
            .program("cursor", |_, p| p
                .vertex("cursor_vert")
                .fragment("cursor_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                ]))
            .program("placement", |_, p| p
                .vertex("placement_vert")
                .fragment("placement_frag")
                .fragment_defines(vec![
                    format!("SELECTION_VALID {}", selection_valid),
                    format!("SELECTION_INVALID {}", selection_invalid),
                ])
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_lookup", 1),
                ]))
            .program("icon", |_, p| p
                .vertex("icon_vert")
                .fragment("icon_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_size", 1),
                    ("attrib_color", 2),
                    ("attrib_texture_info", 3),
                    ("attrib_atlas", 4),
                    ("attrib_vert", 5),
                    ("attrib_uv", 6),
                ]))
            .program("terrain", |_, p| p
                .vertex("terrain_vert")
                .fragment("terrain_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_normal", 1),
                    ("attrib_texture", 2),
                ]))
            .program("terrain_selection", |c, p| {
                let base = c.program("terrain");
                p
                    .vertex("terrain_vert")
                    .fragment("terrain_frag")
                    .vertex_defines(vec!["selection"])
                    .fragment_defines(vec![
                        "selection".to_owned(),
                        format!("SELECTION_VALID {}", selection_valid),
                        format!("SELECTION_INVALID {}", selection_invalid),
                    ])
                    .attribute_binds(&[
                        ("attrib_position", assume!(log, base.attribute("attrib_position")).index()),
                        ("attrib_normal", assume!(log, base.attribute("attrib_normal")).index()),
                        ("attrib_texture", assume!(log, base.attribute("attrib_texture")).index()),
                    ])
            })
            .program("terrain_shadow", |c, p| {
                let base = c.program("terrain");
                p
                    .enabled(shadows_enabled)
                    .vertex("terrain_vert")
                    .vertex_defines(vec!["shadow_pass"])
                    .fragment("terrain_frag")
                    .fragment_defines(vec!["shadow_pass"])
                    .attribute_binds(&[
                        ("attrib_position", assume!(log, base.attribute("attrib_position")).index()),
                        ("attrib_normal", assume!(log, base.attribute("attrib_normal")).index()),
                        ("attrib_texture", assume!(log, base.attribute("attrib_texture")).index()),
                    ])
            })
            .program("static", |_, p| p
                .vertex("static_vert")
                .fragment("static_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_normal", 1),
                    ("attrib_uv", 2),
                    ("attrib_matrix", 3),
                    // 4
                    // 5
                    // 6
                    ("attrib_tint", 7),
                    ("attrib_highlight", 8),
                ]))
            .program("static_water", |c, p| {
                let base = c.program("static");
                p
                    .vertex("static_vert")
                    .fragment("static_frag")
                    .vertex_defines(vec!["water"])
                    .fragment_defines(vec!["water"])
                    .attribute_binds(&[
                        ("attrib_position", assume!(log, base.attribute("attrib_position")).index()),
                        ("attrib_normal", assume!(log, base.attribute("attrib_normal")).index()),
                        ("attrib_uv", assume!(log, base.attribute("attrib_uv")).index()),
                        ("attrib_matrix", assume!(log, base.attribute("attrib_matrix")).index()),
                        ("attrib_tint", assume!(log, base.attribute("attrib_tint")).index()),
                        ("attrib_highlight", assume!(log, base.attribute("attrib_highlight")).index()),
                    ])
            })
            .program("static_shadow", |c, p| {
                let base = c.program("static");
                p
                    .enabled(shadows_enabled)
                    .vertex("static_vert")
                    .vertex_defines(vec!["shadow_pass"])
                    .fragment("static_frag")
                    .fragment_defines(vec!["shadow_pass"])
                    .attribute_binds(&[
                        ("attrib_position", assume!(log, base.attribute("attrib_position")).index()),
                        ("attrib_normal", assume!(log, base.attribute("attrib_normal")).index()),
                        ("attrib_uv", assume!(log, base.attribute("attrib_uv")).index()),
                        ("attrib_matrix", assume!(log, base.attribute("attrib_matrix")).index()),
                        ("attrib_tint", assume!(log, base.attribute("attrib_tint")).index()),
                        ("attrib_highlight", assume!(log, base.attribute("attrib_highlight")).index()),
                    ])
            })
            .program("static_highlight", |c, p| {
                let base = c.program("static");
                p
                    .vertex("static_vert")
                    .vertex_defines(vec!["highlight"])
                    .fragment("static_frag")
                    .fragment_defines(vec!["highlight"])
                    .attribute_binds(&[
                        ("attrib_position", assume!(log, base.attribute("attrib_position")).index()),
                        ("attrib_normal", assume!(log, base.attribute("attrib_normal")).index()),
                        ("attrib_uv", assume!(log, base.attribute("attrib_uv")).index()),
                        ("attrib_matrix", assume!(log, base.attribute("attrib_matrix")).index()),
                        ("attrib_tint", assume!(log, base.attribute("attrib_tint")).index()),
                        ("attrib_highlight", assume!(log, base.attribute("attrib_highlight")).index()),
                    ])
            })
            .program("animated", |_, p| p
                .vertex("animated_vert")
                .fragment("animated_frag")
                .attribute_binds(&[
                    ("attrib_position", 0),
                    ("attrib_normal", 1),
                    ("attrib_uv", 2),
                    ("attrib_bones", 3),
                    ("attrib_bone_weights", 4),
                    ("attrib_matrix", 5),
                    // 6
                    // 7
                    // 8
                    ("attrib_tint", 9),
                    ("attrib_highlight", 10),
                    ("attrib_bone_offset", 11),
                    ("attrib_tint_offset", 12),
                ]))
            .program("animated_shadow", |c, p| {
                let base = c.program("animated");
                p
                    .enabled(shadows_enabled)
                    .vertex("animated_vert")
                    .vertex_defines(vec!["shadow_pass"])
                    .fragment("animated_frag")
                    .fragment_defines(vec!["shadow_pass"])
                    .attribute_binds(&[
                        ("attrib_position", assume!(log, base.attribute("attrib_position")).index()),
                        ("attrib_normal", assume!(log, base.attribute("attrib_normal")).index()),
                        ("attrib_uv", assume!(log, base.attribute("attrib_uv")).index()),
                        ("attrib_bones", assume!(log, base.attribute("attrib_bones")).index()),
                        ("attrib_bone_weights", assume!(log, base.attribute("attrib_bone_weights")).index()),
                        ("attrib_matrix", assume!(log, base.attribute("attrib_matrix")).index()),
                        ("attrib_tint", assume!(log, base.attribute("attrib_tint")).index()),
                        ("attrib_highlight", assume!(log, base.attribute("attrib_highlight")).index()),
                        ("attrib_bone_offset", assume!(log, base.attribute("attrib_bone_offset")).index()),
                        ("attrib_tint_offset", assume!(log, base.attribute("attrib_tint_offset")).index()),
                    ])
            })
            .program("animated_highlight", |c, p| {
                let base = c.program("animated");
                p
                    .vertex("animated_vert")
                    .vertex_defines(vec!["highlight"])
                    .fragment("animated_frag")
                    .fragment_defines(vec!["highlight"])
                    .attribute_binds(&[
                        ("attrib_position", assume!(log, base.attribute("attrib_position")).index()),
                        ("attrib_normal", assume!(log, base.attribute("attrib_normal")).index()),
                        ("attrib_uv", assume!(log, base.attribute("attrib_uv")).index()),
                        ("attrib_bones", assume!(log, base.attribute("attrib_bones")).index()),
                        ("attrib_bone_weights", assume!(log, base.attribute("attrib_bone_weights")).index()),
                        ("attrib_matrix", assume!(log, base.attribute("attrib_matrix")).index()),
                        ("attrib_tint", assume!(log, base.attribute("attrib_tint")).index()),
                        ("attrib_highlight", assume!(log, base.attribute("attrib_highlight")).index()),
                        ("attrib_bone_offset", assume!(log, base.attribute("attrib_bone_offset")).index()),
                        ("attrib_tint_offset", assume!(log, base.attribute("attrib_tint_offset")).index()),
                    ])
            })
            .pass("shadow", |_c, p| p
                .enabled(shadows_enabled)
                .size(shadow_size, shadow_size)
                .program_replace("terrain", "terrain_shadow")
                .program_replace("terrain_selection", "terrain_shadow")
                .program_replace("static", "static_shadow")
                .program_replace("static_water", "static_shadow")
                .program_replace("animated", "animated_shadow")
                .attachment("depth", |a| a
                    .ty(pipeline::Type::Float)
                    .components(pipeline::Components::Depth24)
                    .target(pipeline::AttachTarget::Depth)
                    .border_color([0.0, 0.0, 0.0, 1.0])
                    .clear_value([0.0, 0.0, 0.0, 0.0])
                    .compare_ref()
                    .linear())
                .flag(PassFlag::NO_ICONS | PassFlag::NO_CURSOR))
            .pass("highlight", |_c, p| p
                .program_replace("static", "static_highlight")
                .program_replace("static_water", "static_highlight")
                .program_replace("animated", "animated_highlight")
                .attachment("highlights", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::RGB)
                    .target(pipeline::AttachTarget::Color)
                    .clear_value([0.0, 0.0, 0.0, 1.0])
                    .linear())
                .flag(PassFlag::NO_ICONS | PassFlag::NO_CURSOR | PassFlag::NO_TERRAIN | PassFlag::HIGHLIGHTS))
            .pass("blur_highlight_horz", |_, p| p
                .attachment("blur_color", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::RGB)
                    .target(pipeline::AttachTarget::Color)
                    .linear()
                    .clear_value([0.0, 0.0, 0.0, 1.0]))
                .fullscreen(|f| f
                    .shader("blur_color_horz")
                    .input("g_input", "highlight", "highlights"))
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("blur_highlight_vert", |_, p| p
                .attachment("blur_color", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::RGB)
                    .target(pipeline::AttachTarget::Color)
                    .linear()
                    .clear_value([0.0, 0.0, 0.0, 1.0]))
                .fullscreen(|f| f
                    .shader("blur_color_vert")
                    .input("g_input", "blur_highlight_horz", "blur_color"))
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("base", |_c, p| p
                .attachment("color", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::RGB)
                    .target(pipeline::AttachTarget::Color)
                    .linear()
                    .clear_value([0.0, 0.0, 0.0, 1.0]))
                .attachment("normal", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::RGB)
                    .target(pipeline::AttachTarget::Color)
                    .clear_value([0.0, 0.0, 0.0, 0.0]))
                .attachment("position", |a| a
                    .ty(pipeline::Type::Float)
                    .components(pipeline::Components::RGB)
                    .target(pipeline::AttachTarget::Color)
                    .clear_value([0.0, 0.0, 0.0, 0.0]))
                .attachment("depth", |a| a
                    .ty(pipeline::Type::Float)
                    .components(pipeline::Components::Depth24Stencil8)
                    .target(pipeline::AttachTarget::Depth)
                    .clear_value([0.0, 0.0, 0.0, 0.0]))
                .clear_flags(gl::BufferBit::COLOR | gl::BufferBit::DEPTH)
                .flag(PassFlag::empty()))
            .pass("ssao", |_, p| p
                .enabled(ssao_enabled)
                .scale(2)
                .attachment("ssao", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::R)
                    .target(pipeline::AttachTarget::Color)
                    .scale(2)
                    .linear()
                    .clear_value([0.0, 0.0, 0.0, 1.0]))
                .fullscreen(|f| f
                    .shader("ssao")
                    .input("g_position", "base", "position")
                    .input("g_normal", "base", "normal")
                    .pre(move |ctx| {
                        let projection = *ctx.var::<Matrix4<f32>>("projection").expect("Missing projection matrix");
                        let p = ctx.program("ssao");
                        gl::active_texture(1);
                        noise_tex.bind(gl::TextureTarget::Texture2D);
                        p.uniform("noise").map(|v| v.set_int(1));
                        gl::active_texture(0);
                        p.uniform("samples[0]").map(|v| v.set_vec3_array(&ssao_kernel));
                        p.uniform("projection").map(|v| v.set_matrix4(&projection));
                    }))
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("blur_ssao_horz", |_, p| p
                .enabled(ssao_enabled)
                .scale(2)
                .attachment("blur_ssao", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::R)
                    .target(pipeline::AttachTarget::Color)
                    .scale(2)
                    .linear()
                    .clear_value([0.0, 0.0, 0.0, 1.0]))
                .fullscreen(|f| f
                    .shader("blur_ssao_horz")
                    .input("g_ssao", "ssao", "ssao"))
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("blur_ssao_vert", |_, p| p
                .enabled(ssao_enabled)
                .scale(2)
                .attachment("blur_ssao", |a| a
                    .ty(pipeline::Type::U8)
                    .components(pipeline::Components::R)
                    .target(pipeline::AttachTarget::Color)
                    .scale(2)
                    .linear()
                    .clear_value([0.0, 0.0, 0.0, 1.0]))
                .fullscreen(|f| f
                    .shader("blur_ssao_vert")
                    .input("g_ssao", "blur_ssao_horz", "blur_ssao"))
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("merge", |_, p| p
                .final_color()
                .fullscreen(|f| f
                    .shader("merge")
                    .input("g_color", "base", "color")
                    .input("g_normal", "base", "normal")
                    .input("g_position", "base", "position")
                    .input("g_highlight", "highlight", "highlights")
                    .input("g_highlight_blur", "blur_highlight_vert", "blur_color")
                    .when(shadows_enabled, |f| f
                        .input("shadow_map", "shadow", "depth"))
                    .when(ssao_enabled, |f| f
                        .input("g_ssao", "blur_ssao_vert", "blur_ssao")
                    )
                    .pre(move |ctx| {
                        let view_matrix_inv = ctx.var::<Matrix4<f32>>("view_matrix")
                            .and_then(|v| v.invert())
                            .expect("Missing view matrix");
                        let shadow_projection = ctx.var::<Matrix4<f32>>("shadow_projection")
                            .cloned();
                        let shadow_view_matrix = ctx.var::<Matrix4<f32>>("shadow_view_matrix")
                            .cloned();
                        let p = ctx.program("merge");
                        shadow_projection.map(|m|
                            p.uniform("shadow_projection").map(|v| v.set_matrix4(&m))
                        );
                        shadow_view_matrix.map(|m|
                            p.uniform("shadow_matrix").map(|v| v.set_matrix4(&m))
                        );
                        p.uniform("view_matrix_inv").map(|v| v.set_matrix4(&view_matrix_inv));
                    })
                )
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("focused_effect",  |_, p| p
                .final_color()
                .runtime_enable("focused")
                .fullscreen(|f| f
                    .shader("focused_effect")
                    .input_final("g_color")
                    .input("g_position", "base", "position")
                    .pre(move |ctx| {
                        let time = *ctx.var::<f64>("time").expect("Missing timer");
                        let view_matrix_inv = ctx.var::<cgmath::Matrix4<f32>>("view_matrix")
                            .and_then(|v| v.invert())
                            .expect("Missing view matrix");
                        let area = *ctx.var::<Option<Bound>>("focused_area")
                            .and_then(|v| v.as_ref())
                            .expect("Missing focused_area");
                        let p = ctx.program("focused_effect");
                        p.uniform("time").map(|v| v.set_float(time as f32));
                        p.uniform("focused_area").map(|v| v.set_int4(area.min.x, area.min.y, area.max.x, area.max.y));
                        p.uniform("view_matrix_inv").map(|v| v.set_matrix4(&view_matrix_inv));
                    })
                )
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("fxaa", |_, p| p
                .enabled(fxaa_enabled)
                .final_color()
                .fullscreen(|f| f
                    .shader("fxaa")
                    .input_final("g_color"))
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("scale", |_, p| p
                .enabled(render_scaling)
                .final_color()
                .fullscreen(|f| f
                    .shader("scale")
                    .input_final("g_color")
                )
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .pass("pause_effect",  |_, p| p
                .final_color()
                .runtime_enable("paused")
                .fullscreen(|f| f
                    .shader("pause_effect")
                    .input_final("g_color")
                    .pre(move |ctx| {
                        let time = *ctx.var::<f64>("time").expect("Missing timer");
                        let p = ctx.program("pause_effect");
                        p.uniform("time").map(|v| v.set_float(time as f32));
                    })
                )
                .clear_flags(gl::BufferBit::COLOR)
                .flag(PassFlag::empty()))
            .build()
    }

    /// Initializes the user interface
    pub fn init_ui(&mut self, manager: &mut ::fungui::Manager<UniverCityUI>) {
        self.ui_renderer.init(manager);
        self.ui_renderer.layout(manager, 800, 480);
    }

    /// Draws the user interface
    pub fn draw_ui(&mut self, manager: &mut ::fungui::Manager<UniverCityUI>) {
        self.ui_renderer.draw(manager, &mut self.pipeline.context(), &mut self.state.global_atlas);
    }

    /// Changes the render ui scale
    pub fn set_ui_scale(&mut self, scale: f32) {
        self.ui_renderer.set_scale(scale);
    }
    /// Changes the render ui scale
    pub fn ui_scale(&mut self) -> f32 {
        self.ui_renderer.ui_scale
    }

    /// Replaces/creates the named image with the passed data
    pub fn update_image(&mut self,
        name: ResourceKey<'_>,
        width: u32, height: u32, data: Vec<u8>
    ) {
        self.ui_renderer.update_image(&mut self.state.global_atlas, name, width, height, data)
    }

    /// Ticks the renderer, updating state and drawing objects to the screen
    /// as required.
    pub fn tick(&mut self,
        mut entities: Option<&mut ecs::Container>,
        manager: Option<&mut ::fungui::Manager<UniverCityUI>>,
        delta: f64,
        width: u32, height: u32,
    ) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
        }
        self.handle_textures();
        manager.map(|v| self.ui_renderer.layout(v, width, height));

        // Move the camera if required
        self.camera_update(delta);

        let view_matrix = RenderState::get_view_matrix(
            self.camera.x, self.camera.y,
            self.camera.zoom, self.camera.rotation,
        );
        let projection = RenderState::get_projection_matrix (
            width, height, self.camera.zoom
        );
        let frustum = Frustum::from_matrix(projection * view_matrix);
        let (tx, ty, ts) = self.terrain.as_ref()
            .map_or(Default::default(), |v| v.get_render_bounds(&frustum));
        let shadow_view_matrix = RenderState::get_view_matrix(
            tx, ty,
            1.0, self.shadow_rotation,
        );
        let shadow_projection: cgmath::Matrix4<f32> = cgmath::ortho(
            (-ts * 0.5) * 70.0,
            (ts * 0.5) * 70.0,
            (-ts * 0.25) * 70.0,
            (ts * 0.25) * 70.0,
            -(ts * 0.5) * 70.0,
            (ts * 0.5) * 70.0,
        );
        gl::active_texture(GLOBAL_TEXTURE_LOCATION);
        self.global_atlas.texture.bind(gl::TextureTarget::Texture2DArray);

        gl::Program::unbind();
        gl::Framebuffer::unbind(gl::TargetFramebuffer::Both);

        gl::view_port(0, 0, width, height);
        gl::clear_color(0.0, 0.0, 0.0, 1.0);
        gl::clear(gl::BufferBit::COLOR | gl::BufferBit::DEPTH);

        // Entity frame building
        if let Some(entities) = entities.as_mut() {
            self.compute_entities(entities, &frustum, delta);
        }
        {
            self.time += delta;
            if self.time > 1_048_575.0 {
                self.time -= 1_048_575.0;
            }
            let time = self.time;
            let paused = self.paused;
            let state = &mut self.state;
            let focused_area = state.focused_region;
            self.pipeline
                .begin_draw()
                .var("paused", paused)
                .var("time", time)
                .var("focused", focused_area.is_some())
                .var("focused_area", focused_area)
                .var("frustum", frustum)
                .var("view_matrix", view_matrix)
                .var("projection", projection)
                .pass_var("shadow", "view_matrix", shadow_view_matrix)
                .pass_var("shadow", "projection", shadow_projection)
                .pass_var("highlight", "view_matrix", view_matrix)
                .pass_var("highlight", "projection", projection)
                .pass_var("base", "view_matrix", view_matrix)
                .pass_var("base", "projection", projection)
                .pass_var("base", "shadow_view_matrix", shadow_view_matrix)
                .pass_var("base", "shadow_projection", shadow_projection)
                .draw(width, height, |ctx, flag| state.draw(ctx, *flag, &mut entities, delta));

            if let Some(entities) = entities.as_mut() {
                gl::clear(gl::BufferBit::DEPTH);
                state.draw_icons(&mut self.pipeline.context(), &projection, &view_matrix, entities);
            }
        }

        gl::view_port(0, 0, width, height);

        let input = self.video_system.text_input();
        if !self.is_text_input && input.is_active() {
            input.stop();
            self.text_input_region = None;
        }
        self.is_text_input = false;
    }

    /// Marks a region as taking text input this frame.
    ///
    /// Should be called every frame as required
    pub fn mark_text_input(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let input = self.video_system.text_input();
        if !self.is_text_input {
            self.is_text_input = true;
            input.start();
        }
        if self.text_input_region != Some((x, y, w, h)) {
            self.text_input_region = Some((x, y, w, h));
            input.set_rect(::sdl2::rect::Rect::new(x, y, w as u32, h as u32));
        }
    }

    /// Sets the level currently being rendered by this renderer.
    pub fn set_level(&mut self, level: &level::Level) {
        self.terrain = None;
        self.terrain = Some(terrain::Terrain::new(&self.state.log, &self.state.asset_manager, level, &mut self.pipeline.context()));
        self.camera.zoom = 0.8;
    }

    /// Removes the level currently being rendered by this renderer
    pub fn clear_level(&mut self) {
        self.terrain = None;
    }

    /// Updates the internal state of the level for the renderer.
    pub fn update_level(&mut self, level: &mut level::Level) {
        if let Some(t) = self.state.terrain.as_mut() {
            t.update(&mut self.pipeline.context(), &mut self.state.global_atlas, level);
        }
    }

    /// Marks a region to be focused
    pub fn set_focused_region(&mut self, region: Bound) {
        self.focused_region = Some(region);
    }
    /// Clears any currently focused region
    pub fn clear_focused_region(&mut self) {
        self.focused_region = None;
    }

    /// Marks a region to have its walls lowered
    pub fn set_lowered_region(&mut self, region: Bound) {
        if let Some(t) = self.state.terrain.as_mut() {
            t.lowered_region = Some(region);
        }
    }
    /// Gets the currently lowered region if any
    pub fn get_lowered_region(&self) -> Option<Bound> {
        if let Some(t) = self.state.terrain.as_ref() {
            t.lowered_region
        } else {
            None
        }
    }

    /// Clears a region of having its walls lowered
    pub fn clear_lowered_region(&mut self) {
        if let Some(t) = self.state.terrain.as_mut() {
            t.lowered_region = None;
        }
    }

    /// Begins drawing a selection area on the map
    pub fn start_selection<'a>(&mut self, owner_id: player::Id, level: &mut level::Level, room: assets::ResourceKey<'a>, x: i32, y: i32) {
        let bound = Bound::new(
            Location::new(x, y),
            Location::new(x, y)
        );
        let verts = RenderState::gen_selection_verts(&mut self.state.global_atlas, assume!(self.state.log, self.state.terrain.as_mut()), level, room, bound, owner_id);
        self.selection = Some(Selection {
            cycle: 0.0,
            selection_model: model::Model::new(
                &mut self.pipeline.context(),
                "terrain_selection",
                vec![
                    model::Attribute{name: "attrib_position", count: 3, ty: gl::Type::Float, offset: 0, int: false},
                    model::Attribute{name: "attrib_normal", count: 3, ty: gl::Type::Float, offset: 12, int: false},
                    model::Attribute{name: "attrib_texture", count: 4, ty: gl::Type::Float, offset: 24, int: false},
                ],
                verts
            ),
                start: Location::new(x, y),
                current: Location::new(x, y),
        });
        let mdl = self.selection.as_mut().expect("Missing selection after setting");
        mdl.selection_model.uniforms.insert("u_cycle", model::UniformValue::Float(0.0));
        mdl.selection_model.uniforms.insert("u_textures", model::UniformValue::Int(GLOBAL_TEXTURE_LOCATION as i32));
        mdl.selection_model.uniforms.insert("shadow_map", model::UniformValue::Int(SHADOW_MAP_LOCATION as i32));
        mdl.selection_model.uniforms.insert("shadow_matrix", model::UniformValue::Matrix4(cgmath::SquareMatrix::identity()));
        mdl.selection_model.uniforms.insert("shadow_projection", model::UniformValue::Matrix4(cgmath::SquareMatrix::identity()));
    }

    /// Ends the selection area and stops drawing it.
    pub fn stop_selection(&mut self, _: &mut level::Level, _: i32, _: i32) {
        self.selection = None;
    }

    /// Move the selection position
    pub fn move_selection<'a>(&mut self, owner_id: player::Id, level: &mut level::Level, room: assets::ResourceKey<'a>, x: i32, y: i32) {
        if let Some(selection) = self.state.selection.as_mut() {
            let pos = Location::new(x, y);
            if selection.current != pos {
                selection.current = pos;
                let bound = Bound::new(selection.start, selection.current);
                let verts = RenderState::gen_selection_verts(&mut self.state.global_atlas, assume!(self.state.log, self.state.terrain.as_mut()), level, room, bound, owner_id);
                selection.selection_model = model::Model::new(
                    &mut self.pipeline.context(),
                    "terrain_selection",
                    vec![
                        model::Attribute{name: "attrib_position", count: 3, ty: gl::Type::Float, offset: 0, int: false},
                        model::Attribute{name: "attrib_normal", count: 3, ty: gl::Type::Float, offset: 12, int: false},
                        model::Attribute{name: "attrib_texture", count: 4, ty: gl::Type::Float, offset: 24, int: false},
                    ],
                    verts
                );
                selection.selection_model.uniforms.insert("u_cycle", model::UniformValue::Float(0.0));
                selection.selection_model.uniforms.insert("u_textures", model::UniformValue::Int(GLOBAL_TEXTURE_LOCATION as i32));
                selection.selection_model.uniforms.insert("shadow_map", model::UniformValue::Int(SHADOW_MAP_LOCATION as i32));
                selection.selection_model.uniforms.insert("shadow_matrix", model::UniformValue::Matrix4(cgmath::SquareMatrix::identity()));
                selection.selection_model.uniforms.insert("shadow_projection", model::UniformValue::Matrix4(cgmath::SquareMatrix::identity()));
            }
        }
    }
}

impl RenderState {

    fn draw(&mut self,
            ctx: &mut pipeline::Context<'_>, flag: PassFlag,
            entities: &mut Option<&mut ecs::Container>, delta: f64,
    ) {
        let frustum = assume!(self.log, ctx.var::<Frustum>("frustum")).clone();
        let projection = *assume!(self.log, ctx.var::<Matrix4<f32>>("projection"));
        let view_matrix = *assume!(self.log, ctx.var::<Matrix4<f32>>("view_matrix"));
        let shadow_projection = ctx.var::<Matrix4<f32>>("shadow_projection").cloned();
        let shadow_view_matrix = ctx.var::<Matrix4<f32>>("shadow_view_matrix").cloned();

        if let Some(ter) = self.terrain.as_mut() {
            if !flag.contains(PassFlag::NO_TERRAIN) {
                ter.draw(ctx, &frustum, &projection, &view_matrix, shadow_view_matrix.as_ref(), shadow_projection.as_ref());
            }
        }

        // Entity rendering
        if entities.is_some() {
            self.render_entities(ctx, &projection, &view_matrix, shadow_view_matrix.as_ref(), shadow_projection.as_ref(), flag);
        }

        if !flag.contains(PassFlag::NO_CURSOR) {
            if let (Some(shadow_view_matrix), Some(shadow_projection)) = (shadow_view_matrix, shadow_projection) {
                gl::enable(gl::Flag::StencilTest);
                gl::stencil_func(gl::Func::Always, 1, 0xFF);
                gl::stencil_op(gl::StencilOp::Keep, gl::StencilOp::Replace, gl::StencilOp::Keep);
                gl::stencil_mask(0xFF);
                gl::clear_stencil(0);
                gl::clear_bufferi(gl::TargetBuffer::Stencil, 0, &[0]);
                if let Some(sel) = self.selection.as_mut() {
                    sel.cycle += delta;
                    sel.cycle %= 200.0;
                    *assume!(self.log, sel.selection_model.uniforms.get_mut("shadow_matrix")) = model::UniformValue::Matrix4(shadow_view_matrix);
                    *assume!(self.log, sel.selection_model.uniforms.get_mut("shadow_projection")) = model::UniformValue::Matrix4(shadow_projection);
                    *assume!(self.log, sel.selection_model.uniforms.get_mut("u_cycle")) = model::UniformValue::Float(sel.cycle as f32);
                    sel.selection_model.draw(ctx, &projection, &view_matrix);
                } else if self.cursor_visible {
                    self.cursor.draw(ctx, &projection, &view_matrix);
                }

                gl::depth_func(gl::Func::Always);
                gl::stencil_func(gl::Func::Equal, 1, 0xFF);
                gl::stencil_mask(0x00);
                gl::enable_i(gl::Flag::Blend, 0);
                gl::blend_func(gl::BlendFunc::SrcAlpha, gl::BlendFunc::One);

                if let Some(sel) = self.selection.as_mut() {
                    sel.selection_model.draw(ctx, &projection, &view_matrix);
                } else if self.cursor_visible {
                    self.cursor.draw(ctx, &projection, &view_matrix);
                }
                gl::disable(gl::Flag::StencilTest);
                gl::disable_i(gl::Flag::Blend, 0);
                gl::depth_func(gl::Func::GreaterOrEqual);
            }
        }
    }

    fn keyaction_to_dir(action: keybinds::KeyAction) -> usize {
        use crate::keybinds::KeyAction::*;
        match action {
            RenderCameraUp | RenderCameraUpStop => 1,
            RenderCameraLeft | RenderCameraLeftStop => 2,
            RenderCameraRight | RenderCameraRightStop => 3,
            RenderCameraDown | RenderCameraDownStop | _ => 0,
        }
    }

    /// Handles actions from key presses
    pub fn handle_key_action(&mut self, action: keybinds::KeyAction) {
        use crate::keybinds::KeyAction::*;
        match action {
            RenderZoomOut => {
                self.camera.zoom -= 0.05;
                if self.camera.zoom <= MAX_ZOOM_OUT {
                    self.camera.zoom = MAX_ZOOM_OUT;
                }
            },
            RenderZoomIn => {
                self.camera.zoom += 0.05;
                if self.camera.zoom >= MAX_ZOOM_IN {
                    self.camera.zoom = MAX_ZOOM_IN;
                }
            },
            RenderRotateLeft => {
                self.camera.rotation -= cgmath::Deg(45.0);
            },
            RenderRotateRight => {
                self.camera.rotation += cgmath::Deg(45.0);
            },
            RenderCameraDown |
            RenderCameraUp |
            RenderCameraLeft |
            RenderCameraRight => {
                self.camera.movement[Self::keyaction_to_dir(action)] = true;
                self.camera.target = None;
            },
            RenderCameraDownStop |
            RenderCameraUpStop |
            RenderCameraLeftStop |
            RenderCameraRightStop => {
                self.camera.movement[Self::keyaction_to_dir(action)] = false;
            },
            _ => {},
        }
    }

    fn camera_update(&mut self, delta: f64) {
        use crate::keybinds::KeyAction::*;

        if let Some(mut tar) = self.camera.target.take() {
            tar.2 -= delta;
            if tar.2 > 0.0 {
                let dx = tar.0 - self.camera.x;
                let dy = tar.1 - self.camera.y;
                let am = tar.2.min(delta) as f32;
                self.camera.x += (dx / tar.2 as f32) * am;
                self.camera.y += (dy / tar.2 as f32) * am;
                self.camera.target = Some(tar);
                return;
            } else {
                self.camera.x = tar.0;
                self.camera.y = tar.1;
            }
        }

        let mut dx = 0.0;
        let mut dy = 0.0;

        const SPEED: f64 = 0.12;

        if self.camera.movement[Self::keyaction_to_dir(RenderCameraDown)] {
            dy -= SPEED;
        }
        if self.camera.movement[Self::keyaction_to_dir(RenderCameraUp)] {
            dy += SPEED;
        }
        if self.camera.movement[Self::keyaction_to_dir(RenderCameraLeft)] {
            dx += SPEED;
        }
        if self.camera.movement[Self::keyaction_to_dir(RenderCameraRight)] {
            dx -= SPEED;
        }
        self.move_camera((dx * delta) as f32, (dy * delta) as f32);
    }

    fn handle_textures(&mut self) {
        let texture = &self.global_atlas.texture;
        let log = &self.log;
        self.global_atlas.loading_textures.retain(|v| {
            if let Err(err) = v.0.error() {
                error!(log, "Failed to load texture: {:?}", v.0.key(); "error" => ?err);
                return false;
            }
            if let Some(img) = v.0.take_image() {
                Self::upload_texture(texture, Some(img), v.1, v.2);
                return false;
            }
            true
        });
    }

    /// Requests that the render starts loading the texture now
    pub fn preload_texture(&mut self, tex: assets::ResourceKey<'_>) {
        Self::texture_info_for(
            &self.log,
            &self.asset_manager,
            &mut self.global_atlas,
            tex
        );
    }

    fn texture_info_for(
        log: &Logger,
        asset_manager: &assets::AssetManager,
        atlas: &mut GlobalAtlas, tex: assets::ResourceKey<'_>,
    ) -> (i32, atlas::Rect) {
        if let Some(info) = atlas.textures.get(&tex) {
            return *info;
        }

        let img = assume!(log, asset_manager.loader_open::<image::Loader>(tex.borrow()));
        let info = Self::place_texture_atlas(atlas, img);
        atlas.textures.insert(tex.into_owned(), info);
        info
    }

    fn place_texture_atlas(target_atlas: &mut GlobalAtlas, img: image::ImageFuture) -> (i32, atlas::Rect) {
        let (width, height) = img.wait_dimensions().unwrap_or((4, 4));

        for (idx, ref mut atlas) in target_atlas.atlases.iter_mut().enumerate() {
            if let Some(rect) = atlas.find(width as i32, height as i32) {
                let loaded_img = img.take_image();
                let is_not_loaded = loaded_img.is_none();
                Self::upload_texture(&target_atlas.texture, loaded_img, idx as i32, rect);
                if is_not_loaded {
                    target_atlas.loading_textures.push((img, idx as i32, rect));
                }
                return (idx as i32, rect);
            }
        }

        // Resize
        target_atlas.texture.bind(gl::TextureTarget::Texture2DArray);
        // Get the original image data
        let layers = target_atlas.atlases.len();
        let orig = if layers != 0 {
            let mut orig = vec![0; ATLAS_SIZE as usize * ATLAS_SIZE as usize * 4 * layers];
            target_atlas.texture.get_data(gl::TextureTarget::Texture2DArray, 0, gl::TextureFormat::Rgba, gl::Type::UnsignedByte, &mut orig);
            Some(orig)
        } else {
            None
        };
        // Resize the texture
        target_atlas.texture.image_3d(
            gl::TextureTarget::Texture2DArray, 0,
            ATLAS_SIZE as u32, ATLAS_SIZE as u32, (layers + 1) as u32,
            gl::TextureFormat::Srgba8, gl::TextureFormat::Rgba,
            gl::Type::UnsignedByte,
            None
        );
        // Place old data back
        if let Some(orig) = orig {
            target_atlas.texture.sub_image_3d(
                gl::TextureTarget::Texture2DArray, 0,
                0, 0, 0,
                ATLAS_SIZE as u32, ATLAS_SIZE as u32, layers as u32,
                gl::TextureFormat::Rgba, gl::Type::UnsignedByte,
                Some(&orig)
            );
        }
        let mut atlas = atlas::TextureAtlas::new(ATLAS_SIZE, ATLAS_SIZE);
        let rect = atlas.find(width as i32, height as i32).expect("Out of texture space");
        let idx = layers as i32;
        let loaded_img = img.take_image();
        let is_not_loaded = loaded_img.is_none();
        Self::upload_texture(&target_atlas.texture, loaded_img, idx, rect);
        if is_not_loaded {
            target_atlas.loading_textures.push((img, idx as i32, rect));
        }
        target_atlas.atlases.push(atlas);
        (idx, rect)
    }

    fn upload_texture(texture: &gl::Texture, tex: Option<image::Image>, atlas: i32, rect: atlas::Rect) {
        let data = tex.map_or_else(|| vec![255; (rect.width * rect.height * 4) as usize], |v| v.data);
        texture.bind(gl::TextureTarget::Texture2DArray);
        texture.sub_image_3d(
            gl::TextureTarget::Texture2DArray, 0,
            rect.x as u32, rect.y as u32, atlas as u32,
            rect.width as u32, rect.height as u32, 1,
            gl::TextureFormat::Rgba, gl::Type::UnsignedByte,
            Some(&data)
        );
    }
}

#[derive(Clone, Copy)]
struct EntityMatrix<'a> {
    projection: &'a cgmath::Matrix4<f32>,
    view_matrix: &'a cgmath::Matrix4<f32>,
    shadow_view_matrix: Option<&'a cgmath::Matrix4<f32>>,
    shadow_projection: Option<&'a cgmath::Matrix4<f32>>,
}

trait EntityRender<'a> {
    type Component: ecs::Component;
    type Params: ecs::AccessorSet;

    fn clear(&mut self);

    fn compute_frame(&mut self,
        asset_manager: &assets::AssetManager,
        config: &Config,
        global_atlas: &mut GlobalAtlas,

        em: &EntityManager<'_>,
        pos: &Read<Position>,
        size: &Read<Size>,
        model: &Read<entity::Model>,
        model_tex: &Read<entity::ModelTexture>,
        component: &Read<Self::Component>,
        params: &Self::Params,

        model_key: assets::ResourceKey<'_>,
        texture: Option<assets::ResourceKey<'_>>,
        ents: &[ecs::Entity],
        delta: f64,
    );

    fn render(
        &mut self,
        ctx: &mut pipeline::Context<'_>,
        matrix: EntityMatrix<'_>,
        flags: PassFlag,
    );
}

impl RenderState {

    fn render_entities(
        &mut self,
        ctx: &mut pipeline::Context<'_>,
        projection: &cgmath::Matrix4<f32>,
        view_matrix: &cgmath::Matrix4<f32>,
        shadow_view_matrix: Option<&cgmath::Matrix4<f32>>,
        shadow_projection: Option<&cgmath::Matrix4<f32>>,
        flags: PassFlag,
    ) {
        let matrix = EntityMatrix {
            projection,
            view_matrix,
            shadow_view_matrix,
            shadow_projection,
        };
        // Static models
        self.static_info.render(ctx, matrix, flags);
        // Animated models
        self.animated_info.render(ctx, matrix, flags);
    }

    fn compute_entities(
        &mut self,
        entities: &mut ecs::Container,
        frustum: &Frustum,
        delta: f64,
    ) {
        // Static models
        self.static_info.clear();
        Self::compute_entities_for(
            &self.log,
            &mut self.static_info,
            &self.asset_manager,
            &self.config,
            &mut self.global_atlas,
            entities,
            frustum,
            delta,
        );
        // Animated models
        self.animated_info.clear();
        Self::compute_entities_for(
            &self.log,
            &mut self.animated_info,
            &self.asset_manager,
            &self.config,
            &mut self.global_atlas,
            entities,
            frustum,
            delta,
        );

        entities.with(|
            em: EntityManager<'_>,
            attachment: ecs::Read<entity::AttachedTo>,
            e_model: ecs::Read<entity::Model>,
            e_model_tex: ecs::Read<entity::ModelTexture>,
            // TODO: Support animated attachments
            _animated: ecs::Read<entity::AnimatedModel>,
            s_model: ecs::Read<entity::StaticModel>,
        | {
            for (e, attachment) in em.group_mask(&attachment, |m| m.and(&e_model)) {
                if !em.is_valid(attachment.target) {
                    continue;
                }
                let other = assume!(self.log, e_model.get_component(attachment.target));
                let (base, bone) = {
                    let tex = e_model_tex.get_component(attachment.target);
                    let tex = tex.map(|v| v.name.borrow());
                    let key = ModelKeyBorrow(other.name.borrow(), tex);
                    let model = if let Some(m) = self.animated_info.info.models.get(&other.name) {
                        m
                    } else { continue };
                    let other_model = if let Some(m) = self.animated_info.gl_models.get(&key) {
                        m
                    } else { continue };
                    if let Some(dyn_offset) = other_model.entity_map.get(&attachment.target).cloned() {
                        let r#dyn = &other_model.dyn_info[dyn_offset];
                        let bone_id = if let Some(id) = model.bones.get(&attachment.bone) {
                            id
                        } else {
                            warn!(self.log, "Missing bone: {}", attachment.bone);
                            continue
                        };
                        (
                            r#dyn.matrix,
                            other_model.bone_node_info[dyn_offset * model.bones.len() + bone_id],
                        )
                    } else {
                        // Entity wasn't rendered this frame, skip
                        continue;
                    }
                };
                let model = assume!(self.log, e_model.get_component(e));
                let tex = e_model_tex.get_component(e);
                let tex = tex.map(|v| v.name.borrow());
                if s_model.get_component(e).is_some() {
                    let model = if let Some(m) = self.static_info.models.get_mut(&ModelKeyBorrow(model.name.borrow(), tex)) {
                        m
                    } else { continue };
                    if let Some(r#dyn) = model.entity_map.get(&e).cloned() {
                        {
                            let r#dyn = &mut model.dyn_info[r#dyn];
                            r#dyn.matrix = base
                                * bone
                                * attachment.offset
                                * model.info.transform;
                        }
                        model.matrix_buffer.bind(gl::BufferTarget::Array);
                        model.matrix_buffer.set_data_range(gl::BufferTarget::Array, &model.dyn_info[r#dyn..=r#dyn], r#dyn as i32);
                    }
                } else {
                    unimplemented!("Animated attachments not supported")
                }
            }
        });
    }

    fn compute_entities_for<ER>(
        log: &Logger,
        info: &mut ER,
        asset_manager: &assets::AssetManager,
        config: &Config,
        global_atlas: &mut GlobalAtlas,
        entities: &mut ecs::Container,
        frustum: &Frustum,
        delta: f64,
    )
        where ER: for<'a> EntityRender<'a>
    {
        use crate::entity::{Model, ModelTexture};

        entities.with(|
            em: EntityManager<'_>,
            component: Read<ER::Component>,
            pos: Read<Position>,
            size: Read<Size>,
            model: Read<Model>,
            model_tex: Read<ModelTexture>,
            params: ER::Params,
        | {
            let mask = component.mask()
                .and(&model)
                .and(&pos);

            let mut ents = em.iter_mask(&mask)
                .collect::<SmallVec<[_; 32]>>();

            ents.retain(|v| {
                let pos = assume!(log, pos.get_component(*v));
                let pos = cgmath::Vector3::new(
                    pos.x,
                    pos.y,
                    pos.z
                );
                if let Some(size) = size.get_component(*v) {
                    let dims = cgmath::Vector3::new(
                        size.width / 2.0 + 1.5,
                        size.height / 2.0 + 1.5,
                        size.depth / 2.0 + 1.5
                    );
                    frustum.contains_aabb(
                        pos - dims,
                        pos + dims,
                    )
                } else {
                    frustum.contains_sphere(pos, 1.5)
                }
            });
            ents.sort_unstable_by(|ae, be| {
                let a = assume!(log, model.get_component(*ae));
                let b = assume!(log, model.get_component(*be));
                a.name.cmp(&b.name)
                    .then_with(|| {
                        let a = model_tex.get_component(*ae);
                        let b = model_tex.get_component(*be);
                        a.cmp(&b)
                    })
            });

            for eq in ents.groups(|a, b| {
                let a_tex = model_tex.get_component(*a);
                let b_tex = model_tex.get_component(*b);
                let a = assume!(log, model.get_component(*a));
                let b = assume!(log, model.get_component(*b));
                a.name == b.name && a_tex == b_tex
            }) {
                let model_key = assume!(log, model.get_component(eq[0])).name.borrow();
                let tex = model_tex.get_component(eq[0]);
                info.compute_frame(
                    asset_manager,
                    config,
                    global_atlas,

                    &em,
                    &pos,
                    &size,
                    &model,
                    &model_tex,
                    &component,
                    &params,

                    model_key,
                    tex.map(|v| v.name.borrow()),
                    eq,
                    delta
                );
            }
        });
    }
}

impl RenderState {

    /// Returns a ray from the mouse into the world
    pub fn get_mouse_ray(&self, mx: i32, my: i32) -> Ray<f32> {
        use cgmath::{Vector4};
        // Obtain the view matrix and then invert it. This allows
        // us to obtain the position in the world of the screen
        // pixel.
        let view_matrix = Self::get_projection_matrix(self.width, self.height, self.camera.zoom)
            * Self::get_view_matrix(
                self.camera.x, self.camera.y,
                self.camera.zoom, self.camera.rotation,
            );
        let view_matrix = assume!(self.log, view_matrix.invert());

        // Convert the mouse position to clip space [-1, 1]
        let mut cur = Vector4::new(
            ((mx as f32 / self.width as f32) - 0.5) * 2.0,
            -((my as f32 / self.height as f32) - 0.5) * 2.0,
            0.0,
            1.0
        );
        // Use the inverted view matrix to find the location of the pixel
        // in the world
        let start = view_matrix * cur;
        let start = start / start.w;

        // Do the same again but this time away from the screen
        cur.z = 1.0;
        let end = view_matrix * cur;
        let end = end / start.w;

        // Find the direction of our ray by getting the difference
        // between our two lookups
        let l = (end - start).truncate().normalize();

        Ray {
            start: start.truncate() - l * 10.0,
            direction: l,
        }
    }

    /// Converts mouse coordinates to a location in the level.
    pub fn mouse_to_level(&self, mx: i32, my: i32) -> (f32, f32) {
        let Ray{start, direction: l, ..} = self.get_mouse_ray(mx, my);
        // The normal vector of our plane
        let n = cgmath::Vector3::new(0.0, 1.0, 0.0);
        // Cast our ray and find where it hits. We exploit the fact the
        // world is a plane at (0.0, 0.0, 0.0) to simplify this.
        let dist = (-start).dot(n) / (l.dot(n));
        let pos = start + l * dist;
        // Convert to grid coordinates
        (
            pos.x,
            pos.z
        )
    }

    fn get_projection_matrix(width: u32, height: u32, zoom: f32) -> Matrix4<f32> {
        let aspect = (height as f32) / (width as f32);
        let w = 400.0;
        let h = 400.0 * aspect;
        let scale = zoom.max(1.0);
        cgmath::ortho(-w, w, -h, h, -h * 2.0 * scale, h * 2.0 * scale)
    }

    fn get_view_matrix(x: f32, y: f32, zoom: f32, rotation: cgmath::Deg<f32>) -> Matrix4<f32> {
        use cgmath::{Basis3, Matrix3, Deg, Vector3};
        Matrix4::from_scale(75.0 * zoom)
            * Matrix4::from(Matrix3::from(Basis3::from_angle_x(Deg(-35.264f32))))
            * Matrix4::from(Matrix3::from(Basis3::from_angle_y(Deg(-45.0f32) + rotation)))
            * Matrix4::from_translation(Vector3::new(
                -x,
                0.0,
                -y
            ))
    }

    /// Sets the position of the cursor
    pub fn set_mouse_position(&mut self, x: i32, y: i32) {
        self.mouse_pos = (x, y);
    }

    /// Sets the sprite rendered at the mouse position
    pub fn set_mouse_sprite(&mut self, sprite: ResourceKey<'_>) {
        use sdl2::surface::Surface;
        use sdl2::pixels::PixelFormatEnum;
        if self.mouse_sprite != sprite {
            let mut img = assume!(self.log, assume!(self.log, self.asset_manager.loader_open::<image::Loader>(
                sprite.clone()
            )).wait_take_image());

            let sur = assume!(self.log, Surface::from_data(&mut img.data, img.width, img.height, 4 * img.width, PixelFormatEnum::ABGR8888));
            let cur = assume!(self.log, Cursor::from_surface(sur, 0, 0));
            cur.set();

            self.mouse_sprite = sprite.into_owned();
            self.mouse_cursor = Some(cur);
        }
    }

    /// Returns the position of the camera.
    pub fn get_camera(&self) -> (f32, f32) {
        (self.camera.x, self.camera.y)
    }

    /// Returns the rotation and zoom of the camera
    pub fn get_camera_info(&self) -> (cgmath::Deg<f32>, f32) {
        (self.camera.rotation, self.camera.zoom)
    }

    /// Sets the position of the camera.
    pub fn set_camera(&mut self, x: f32, y: f32) {
        self.camera.x = x;
        self.camera.y = y;
    }

    /// Attempts to move the camera to the target location
    /// unless the player overrides
    pub fn suggest_camera_position(&mut self, x: f32, y: f32, time: f64) {
        self.camera.target = Some((x, y, time));
    }

    /// Moves the camera relative to the rotation
    pub fn move_camera(&mut self, x: f32, y: f32) {
        use std::f32::consts::PI;
        let len = (x*x + y*y).sqrt() / self.camera.zoom;
        let ang = -cgmath::Rad::from(self.camera.rotation).0 + y.atan2(x) + (PI / 4.0) * 3.0;
        let c = ang.cos();
        let s = ang.sin();
        self.camera.x -= len * s;
        self.camera.y -= len * c;
        if let Some(terrain) = self.terrain.as_ref() {
            self.camera.x = self.camera.x.min(terrain.width as f32 - 48.0).max(48.0);
            self.camera.y = self.camera.y.min(terrain.height as f32 - 48.0).max(48.0);
        }
        // Override any automatic movement going on
        self.camera.target = None;
    }

    fn gen_selection_verts<'a>(
        target_atlas: &mut GlobalAtlas,
        terrain: &mut terrain::Terrain, level: &mut level::Level,
        room: assets::ResourceKey<'a>, bound: Bound,
        owner_id: player::Id,
    ) -> Vec<terrain::GLVertex> {
        let valid = level.check_room_size(room.borrow(), bound);
        let mut verts = vec![];
        for loc in bound {
            let tile_valid = !(!valid || !level.check_tile_placeable(owner_id, room.borrow(), loc));
            let mut tverts = terrain.gen_verts_for_tile(target_atlas, level, level, loc);
            for vert in &mut tverts {
                vert.brightness = if tile_valid {
                    1.0
                } else {
                    0.0
                };
            }
            verts.extend_from_slice(&tverts);
        }
        verts
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Hash, Clone)]
pub(crate) struct ModelKey {
    pub key: ModelKeyBorrow<'static>
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Hash, Clone)]
pub(crate) struct ModelKeyBorrow<'a>(pub assets::ResourceKey<'a>, pub Option<assets::ResourceKey<'a>>);

impl <'a> Borrow<ModelKeyBorrow<'a>> for ModelKey {
    fn borrow(&self) -> &ModelKeyBorrow<'a> {
        &self.key
    }
}
impl Deref for ModelKey {
    type Target = ModelKeyBorrow<'static>;
    fn deref(&self) -> &ModelKeyBorrow<'static> {
        &self.key
    }
}