
use super::*;
use crate::render::exmodel;
use crate::render;
use cgmath::{InnerSpace, Vector4, Vector3, Matrix4, Decomposed, Quaternion, Deg, Rotation3};

pub(super) struct Model {
    pub verts_x: Vec<GLVertex>,
    pub verts_z: Vec<GLVertex>,
}

impl Model {
    pub(super) fn load(log: &Logger,
                assets: &AssetManager,
                target_atlas: &mut render::GlobalAtlas,
                key: ResourceKey<'_>) -> Model
    {
        let mut file = assume!(log, assets.open_from_pack(key.module_key(), &format!("models/{}.umod", key.resource())));
        let minfo = assume!(log, exmodel::Model::read_from(&mut file));

        let mut mdl = Model {
            verts_x: Vec::new(),
            verts_z: Vec::new(),
        };

        let xtrans: Matrix4<f32> = Decomposed {
            scale: 0.1,
            rot: Quaternion::from_angle_x(Deg(90.0)),
            disp: Vector3::new(1.0, 0.0, 0.5),
        }.into();
        let xtrans = xtrans * minfo.transform;

        let ztrans: Matrix4<f32> = Decomposed {
            scale: 0.1,
            rot: Quaternion::from_angle_y(Deg(-90.0))
                * Quaternion::from_angle_x(Deg(90.0)),
            disp: Vector3::new(0.5, 0.0, 1.0),
        }.into();
        let ztrans = ztrans * minfo.transform;

        let mut sub = minfo.sub_textures.iter()
            .map(|v| (v.0, LazyResourceKey::parse(&v.1)
                .or_module(key.module_key())))
            .collect::<Vec<_>>();
        sub.push((minfo.verts.len(), ResourceKey::new("base", "solid")));

        for face in minfo.faces {
            for i in &face.indices {
                let idx = *i as usize;
                let v = &minfo.verts[idx];

                let tex = assume!(log, sub.windows(2)
                    .find(|v| idx < v[1].0)
                    .map(|v| &v[0]));
                let tex = tex.1.borrow();

                let texture_id = if tex == ResourceKey::new("base", "wall_placeholder") {
                    (-1, Rect { x: 0, y: 0, width: 128, height: 128})
                } else if tex == ResourceKey::new("base", "wall_back_placeholder") {
                    (-2, Rect { x: 0, y: 0, width: 128, height: 128})
                } else if tex == ResourceKey::new("base", "wall_top_placeholder") {
                    (-3, Rect { x: 0, y: 0, width: 128, height: 128})
                } else if tex == ResourceKey::new("base", "wall_side_placeholder") {
                    (-4, Rect { x: 0, y: 0, width: 128, height: 128})
                } else {
                    Terrain::get_texture_id(log, assets, target_atlas, tex)
                };

                let pos = xtrans * Vector4::new(v.x, v.y, v.z, 1.0);
                let normal = (xtrans * Vector4::new(v.nx, v.ny, v.nz, 0.0)).normalize();

                mdl.verts_x.push(GLVertex {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,

                    nx: normal.x,
                    ny: normal.y,
                    nz: normal.z,

                    texture: texture_id.0 as f32,
                    texture_offset_x: texture_id.1.x as f32 + v.tx * texture_id.1.width as f32,
                    texture_offset_y: texture_id.1.y as f32 + v.ty * texture_id.1.height as f32,
                    brightness: 1.0,
                });

                let pos = ztrans * Vector4::new(v.x, v.y, v.z, 1.0);
                let normal = (ztrans * Vector4::new(v.nx, v.ny, v.nz, 0.0)).normalize();

                mdl.verts_z.push(GLVertex {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,

                    nx: normal.x,
                    ny: normal.y,
                    nz: normal.z,

                    texture: texture_id.0 as f32,
                    texture_offset_x: texture_id.1.x as f32 + v.tx * texture_id.1.width as f32,
                    texture_offset_y: texture_id.1.y as f32 + v.ty * texture_id.1.height as f32,
                    brightness: 1.0,
                });
            }
        }

        mdl
    }
}
