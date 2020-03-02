extern crate byteorder;
extern crate cgmath;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::io::{Read, Result, Write};

fn write_string<W: Write>(w: &mut W, s: &str) -> Result<()> {
    w.write_u16::<LittleEndian>(s.len() as u16)?;
    w.write_all(s.as_bytes())?;
    Ok(())
}

fn read_string<R: Read>(r: &mut R) -> Result<String> {
    let str_len = r.read_u16::<LittleEndian>()? as usize;
    let mut str = String::with_capacity(str_len);
    r.take(str_len as u64).read_to_string(&mut str)?;
    Ok(str)
}

#[derive(Debug)]
pub struct Model {
    pub texture: String,
    pub sub_textures: Vec<(usize, String)>,
    pub transform: cgmath::Matrix4<f32>,
    pub faces: Vec<Face>,
    pub verts: Vec<Vertex>,
}

impl Model {
    pub fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        write_string(w, &self.texture)?;

        w.write_u32::<LittleEndian>(self.sub_textures.len() as u32)?;
        for &(start, ref tex) in &self.sub_textures {
            w.write_u32::<LittleEndian>(start as u32)?;
            write_string(w, tex)?;
        }

        let floats: &[f32; 16] = self.transform.as_ref();
        for f in floats {
            w.write_f32::<LittleEndian>(*f)?;
        }

        w.write_u32::<LittleEndian>(self.verts.len() as u32)?;
        for vert in &self.verts {
            vert.write_to(w)?;
        }

        let size = if self.verts.len() <= 0xFF {
            IndexSize::U8
        } else if self.verts.len() <= 0xFFFF {
            IndexSize::U16
        } else {
            IndexSize::U32
        };

        w.write_u32::<LittleEndian>(self.faces.len() as u32)?;
        for face in &self.faces {
            face.write_to(w, size)?;
        }
        Ok(())
    }

    pub fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        let texture = read_string(r)?;

        let sub_len = r.read_u32::<LittleEndian>()? as usize;
        let mut sub_textures = vec![];
        for _ in 0..sub_len {
            sub_textures.push((r.read_u32::<LittleEndian>()? as usize, read_string(r)?));
        }

        let mut transform = [0.0f32; 16];
        for f in &mut transform {
            *f = r.read_f32::<LittleEndian>()?;
        }

        let verts_len = r.read_u32::<LittleEndian>()? as usize;
        let mut verts = vec![];
        for _ in 0..verts_len {
            verts.push(Vertex::read_from(r)?);
        }

        let size = if verts.len() <= 0xFF {
            IndexSize::U8
        } else if verts.len() <= 0xFFFF {
            IndexSize::U16
        } else {
            IndexSize::U32
        };

        let faces_len = r.read_u32::<LittleEndian>()? as usize;
        let mut faces = vec![];
        for _ in 0..faces_len {
            faces.push(Face::read_from(r, size)?);
        }

        Ok(Model {
            texture,
            sub_textures,
            transform: unsafe { ::std::mem::transmute(transform) },
            faces,
            verts,
        })
    }
}

#[derive(Clone, Copy)]
enum IndexSize {
    U8,
    U16,
    U32,
}

#[derive(Debug)]
pub struct Face {
    pub indices: [u32; 3],
}

impl Face {
    fn write_to<W>(&self, w: &mut W, idx: IndexSize) -> Result<()>
    where
        W: Write,
    {
        for i in &self.indices {
            match idx {
                IndexSize::U8 => w.write_u8(*i as u8)?,
                IndexSize::U16 => w.write_u16::<LittleEndian>(*i as u16)?,
                IndexSize::U32 => w.write_u32::<LittleEndian>(*i)?,
            }
        }
        Ok(())
    }

    fn read_from<R>(r: &mut R, idx: IndexSize) -> Result<Self>
    where
        R: Read,
    {
        let mut face = Face { indices: [0; 3] };
        for i in &mut face.indices {
            *i = match idx {
                IndexSize::U8 => u32::from(r.read_u8()?),
                IndexSize::U16 => u32::from(r.read_u16::<LittleEndian>()?),
                IndexSize::U32 => r.read_u32::<LittleEndian>()?,
            };
        }
        Ok(face)
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub nx: f32,
    pub ny: f32,
    pub nz: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Vertex {
    fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        w.write_f32::<LittleEndian>(self.x)?;
        w.write_f32::<LittleEndian>(self.y)?;
        w.write_f32::<LittleEndian>(self.z)?;
        w.write_f32::<LittleEndian>(self.nx)?;
        w.write_f32::<LittleEndian>(self.ny)?;
        w.write_f32::<LittleEndian>(self.nz)?;
        w.write_f32::<LittleEndian>(self.tx)?;
        w.write_f32::<LittleEndian>(self.ty)?;
        Ok(())
    }

    fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        Ok(Vertex {
            x: r.read_f32::<LittleEndian>()?,
            y: r.read_f32::<LittleEndian>()?,
            z: r.read_f32::<LittleEndian>()?,
            nx: r.read_f32::<LittleEndian>()?,
            ny: r.read_f32::<LittleEndian>()?,
            nz: r.read_f32::<LittleEndian>()?,
            tx: r.read_f32::<LittleEndian>()?,
            ty: r.read_f32::<LittleEndian>()?,
        })
    }
}

#[derive(Debug)]
pub struct AniModel {
    pub texture: String,
    pub transform: cgmath::Matrix4<f32>,
    pub root_node: AniNode,
    pub faces: Vec<Face>,
    pub verts: Vec<AniVertex>,
    /// Index 0 is equal to the bone 1.
    ///
    /// 0 is used to mark as having no bone attached
    pub bones: Vec<AniBone>,
}

impl AniModel {
    pub fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        write_string(w, &self.texture)?;

        let floats: &[f32; 16] = self.transform.as_ref();
        for f in floats {
            w.write_f32::<LittleEndian>(*f)?;
        }

        w.write_u32::<LittleEndian>(self.verts.len() as u32)?;
        for vert in &self.verts {
            vert.write_to(w)?;
        }

        let size = if self.verts.len() <= 0xFF {
            IndexSize::U8
        } else if self.verts.len() <= 0xFFFF {
            IndexSize::U16
        } else {
            IndexSize::U32
        };

        w.write_u32::<LittleEndian>(self.faces.len() as u32)?;
        for face in &self.faces {
            face.write_to(w, size)?;
        }

        self.root_node.write_to(w)?;

        w.write_u8(self.bones.len() as u8)?;
        for bone in &self.bones {
            bone.write_to(w)?;
        }

        Ok(())
    }

    pub fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        let texture = read_string(r)?;

        let mut transform = [0.0f32; 16];
        for f in &mut transform {
            *f = r.read_f32::<LittleEndian>()?;
        }

        let verts_len = r.read_u32::<LittleEndian>()? as usize;
        let mut verts = vec![];
        for _ in 0..verts_len {
            verts.push(AniVertex::read_from(r)?);
        }

        let size = if verts.len() <= 0xFF {
            IndexSize::U8
        } else if verts.len() <= 0xFFFF {
            IndexSize::U16
        } else {
            IndexSize::U32
        };

        let faces_len = r.read_u32::<LittleEndian>()? as usize;
        let mut faces = Vec::with_capacity(faces_len);
        for _ in 0..faces_len {
            faces.push(Face::read_from(r, size)?);
        }

        let root_node = AniNode::read_from(r)?;

        let bones_len = r.read_u8()? as usize;
        let mut bones = Vec::with_capacity(bones_len);
        for _ in 0..bones_len {
            bones.push(AniBone::read_from(r)?);
        }

        Ok(AniModel {
            texture,
            transform: unsafe { ::std::mem::transmute(transform) },
            faces,
            verts,
            root_node,
            bones,
        })
    }
}

#[derive(Debug)]
pub struct Animation {
    pub duration: f64,
    pub channels: HashMap<String, AnimationDetails>,
    pub root_node: AniNode,
}

impl Animation {
    pub fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        w.write_f64::<LittleEndian>(self.duration)?;
        w.write_u32::<LittleEndian>(self.channels.len() as u32)?;

        let mut vals: Vec<(&String, &AnimationDetails)> = self.channels.iter().collect();
        vals.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in vals {
            write_string(w, k)?;
            v.write_to(w)?;
        }

        self.root_node.write_to(w)?;

        Ok(())
    }

    pub fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        let duration = r.read_f64::<LittleEndian>()?;

        let channels_len = r.read_u32::<LittleEndian>()? as usize;
        let mut channels = HashMap::with_capacity(channels_len);
        for _ in 0..channels_len {
            channels.insert(read_string(r)?, AnimationDetails::read_from(r)?);
        }

        let root_node = AniNode::read_from(r)?;

        Ok(Animation {
            duration,
            channels,
            root_node,
        })
    }
}

#[derive(Debug)]
pub struct AnimationDetails {
    pub position: Vec<(f64, cgmath::Vector3<f32>)>,
    pub rotation: Vec<(f64, cgmath::Quaternion<f32>)>,
    pub scale: Vec<(f64, cgmath::Vector3<f32>)>,
}

impl AnimationDetails {
    fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        w.write_u32::<LittleEndian>(self.position.len() as u32)?;
        for vert in &self.position {
            w.write_f64::<LittleEndian>(vert.0)?;
            w.write_f32::<LittleEndian>(vert.1.x)?;
            w.write_f32::<LittleEndian>(vert.1.y)?;
            w.write_f32::<LittleEndian>(vert.1.z)?;
        }

        w.write_u32::<LittleEndian>(self.rotation.len() as u32)?;
        for rot in &self.rotation {
            w.write_f64::<LittleEndian>(rot.0)?;
            w.write_f32::<LittleEndian>(rot.1.s)?;
            w.write_f32::<LittleEndian>(rot.1.v.x)?;
            w.write_f32::<LittleEndian>(rot.1.v.y)?;
            w.write_f32::<LittleEndian>(rot.1.v.z)?;
        }

        w.write_u32::<LittleEndian>(self.scale.len() as u32)?;
        for scale in &self.scale {
            w.write_f64::<LittleEndian>(scale.0)?;
            w.write_f32::<LittleEndian>(scale.1.x)?;
            w.write_f32::<LittleEndian>(scale.1.y)?;
            w.write_f32::<LittleEndian>(scale.1.z)?;
        }

        Ok(())
    }

    fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        let position_len = r.read_u32::<LittleEndian>()? as usize;
        let mut position = Vec::with_capacity(position_len);
        for _ in 0..position_len {
            position.push((
                r.read_f64::<LittleEndian>()?,
                cgmath::Vector3::new(
                    r.read_f32::<LittleEndian>()?,
                    r.read_f32::<LittleEndian>()?,
                    r.read_f32::<LittleEndian>()?,
                ),
            ));
        }

        let rotation_len = r.read_u32::<LittleEndian>()? as usize;
        let mut rotation = Vec::with_capacity(rotation_len);
        for _ in 0..rotation_len {
            rotation.push((
                r.read_f64::<LittleEndian>()?,
                cgmath::Quaternion::new(
                    r.read_f32::<LittleEndian>()?,
                    r.read_f32::<LittleEndian>()?,
                    r.read_f32::<LittleEndian>()?,
                    r.read_f32::<LittleEndian>()?,
                ),
            ));
        }

        let scale_len = r.read_u32::<LittleEndian>()? as usize;
        let mut scale = Vec::with_capacity(scale_len);
        for _ in 0..scale_len {
            scale.push((
                r.read_f64::<LittleEndian>()?,
                cgmath::Vector3::new(
                    r.read_f32::<LittleEndian>()?,
                    r.read_f32::<LittleEndian>()?,
                    r.read_f32::<LittleEndian>()?,
                ),
            ));
        }

        Ok(AnimationDetails {
            position,
            rotation,
            scale,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AniBone {
    pub name: String,
    pub offset: cgmath::Matrix4<f32>,
}

impl AniBone {
    fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        write_string(w, &self.name)?;

        let floats: &[f32; 16] = self.offset.as_ref();
        for f in floats {
            w.write_f32::<LittleEndian>(*f)?;
        }

        Ok(())
    }

    fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        let name = read_string(r)?;

        let mut transform = [0.0f32; 16];
        for f in &mut transform {
            *f = r.read_f32::<LittleEndian>()?;
        }

        Ok(AniBone {
            name,
            offset: unsafe { ::std::mem::transmute(transform) },
        })
    }
}

#[derive(Debug, Clone)]
pub struct AniNode {
    pub name: String,
    pub child_nodes: Vec<AniNode>,
    pub transform: cgmath::Matrix4<f32>,
}

impl AniNode {
    fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        write_string(w, &self.name)?;

        let floats: &[f32; 16] = self.transform.as_ref();
        for f in floats {
            w.write_f32::<LittleEndian>(*f)?;
        }

        w.write_u8(self.child_nodes.len() as u8)?;
        for node in &self.child_nodes {
            node.write_to(w)?;
        }

        Ok(())
    }

    fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        let name = read_string(r)?;

        let mut transform = [0.0f32; 16];
        for f in &mut transform {
            *f = r.read_f32::<LittleEndian>()?;
        }

        let len = r.read_u8()?;
        let mut child_nodes = Vec::with_capacity(len as usize);
        for _ in 0..len {
            child_nodes.push(AniNode::read_from(r)?);
        }

        Ok(AniNode {
            name,
            transform: unsafe { ::std::mem::transmute(transform) },
            child_nodes,
        })
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct AniVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub nx: f32,
    pub ny: f32,
    pub nz: f32,
    pub tx: f32,
    pub ty: f32,
    pub bones: [u8; 4],
    pub bone_weights: [f32; 4],
}

impl AniVertex {
    fn write_to<W>(&self, w: &mut W) -> Result<()>
    where
        W: Write,
    {
        w.write_f32::<LittleEndian>(self.x)?;
        w.write_f32::<LittleEndian>(self.y)?;
        w.write_f32::<LittleEndian>(self.z)?;
        w.write_f32::<LittleEndian>(self.nx)?;
        w.write_f32::<LittleEndian>(self.ny)?;
        w.write_f32::<LittleEndian>(self.nz)?;
        w.write_f32::<LittleEndian>(self.tx)?;
        w.write_f32::<LittleEndian>(self.ty)?;

        for b in &self.bones {
            w.write_u8(*b)?;
        }
        for bw in &self.bone_weights {
            w.write_f32::<LittleEndian>(*bw)?;
        }

        Ok(())
    }

    fn read_from<R>(r: &mut R) -> Result<Self>
    where
        R: Read,
    {
        Ok(AniVertex {
            x: r.read_f32::<LittleEndian>()?,
            y: r.read_f32::<LittleEndian>()?,
            z: r.read_f32::<LittleEndian>()?,
            nx: r.read_f32::<LittleEndian>()?,
            ny: r.read_f32::<LittleEndian>()?,
            nz: r.read_f32::<LittleEndian>()?,
            tx: r.read_f32::<LittleEndian>()?,
            ty: r.read_f32::<LittleEndian>()?,
            bones: [r.read_u8()?, r.read_u8()?, r.read_u8()?, r.read_u8()?],
            bone_weights: [
                r.read_f32::<LittleEndian>()?,
                r.read_f32::<LittleEndian>()?,
                r.read_f32::<LittleEndian>()?,
                r.read_f32::<LittleEndian>()?,
            ],
        })
    }
}
