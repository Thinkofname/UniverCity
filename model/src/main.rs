extern crate cgmath;
extern crate libc;
extern crate model;

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi;
use std::fs;
use std::marker::PhantomData;
use std::mem;
use std::path::Path;
use std::ptr;

#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case
)]
mod assimp {
    include!(concat!("assimp.rs"));
}

struct Scene {
    scene: *const assimp::aiScene,
}

impl Scene {
    fn import_file(name: &str) -> Scene {
        let name = ffi::CString::new(name).unwrap();
        unsafe {
            let scene = assimp::aiImportFile(
                name.as_ptr(),
                assimp::aiProcess_Triangulate
                    | assimp::aiProcess_JoinIdenticalVertices
                    | assimp::aiProcess_LimitBoneWeights
                    | assimp::aiProcess_ImproveCacheLocality
                    | assimp::aiProcess_TransformUVCoords
                    | assimp::aiProcess_FlipUVs
                    | assimp::aiProcess_FlipWindingOrder
                    | assimp::aiProcess_MakeLeftHanded,
            );
            if scene.is_null() {
                let err = ffi::CStr::from_ptr(assimp::aiGetErrorString());
                panic!("import_file: {}", err.to_string_lossy());
            }

            Scene { scene: scene }
        }
    }

    fn root_transform(&self) -> cgmath::Matrix4<f32> {
        unsafe {
            let root = (*self.scene).mRootNode;

            for off in 0..(*root).mNumChildren {
                let child = *(*root).mChildren.offset(off as isize);
                if (*child).mNumMeshes == 1 {
                    let mut mat = (*child).mTransformation;
                    assimp::aiMultiplyMatrix4(&mut mat, &(*root).mTransformation);
                    assimp::aiTransposeMatrix4(&mut mat);
                    return mem::transmute(mat);
                }
            }

            let mut mat = (*root).mTransformation;
            assimp::aiTransposeMatrix4(&mut mat);
            mem::transmute(mat)
        }
    }

    fn meshes<'a>(&'a self) -> impl Iterator<Item = Mesh<'a>> + 'a {
        unsafe {
            AIter {
                count: ((*self.scene).mNumMeshes) as isize,
                vals: (*self.scene).mMeshes,
                offset: 0,
                conv: Mesh::from,
                _ret: PhantomData,
            }
        }
    }

    fn animations<'a>(&'a self) -> impl Iterator<Item = Animation<'a>> + 'a {
        unsafe {
            AIter {
                count: ((*self.scene).mNumAnimations) as isize,
                vals: (*self.scene).mAnimations,
                offset: 0,
                conv: Animation::from,
                _ret: PhantomData,
            }
        }
    }

    fn texture_mat(&self, idx: usize) -> Option<String> {
        unsafe {
            let mat = *(*self.scene).mMaterials.offset(idx as isize);
            let count = assimp::aiGetMaterialTextureCount(mat, assimp::aiTextureType_DIFFUSE);
            for idx in 0..count {
                let mut name: assimp::aiString = Default::default();
                if assimp::aiGetMaterialTexture(
                    mat,
                    assimp::aiTextureType_DIFFUSE,
                    idx,
                    &mut name,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                ) != 0
                {
                    panic!("Error")
                }
                let name = ffi::CStr::from_ptr(name.data.as_ptr());
                let name = name.to_string_lossy();
                let pos = name.find("assets/base/base/textures/").expect(&format!(
                    "Texture must be in the textures folder: {:?}",
                    name
                ));

                return Some(
                    name[pos + "assets/base/base/textures/".len()..name.len() - 4].to_owned(),
                );
            }
            None
        }
    }

    fn texture(&self) -> String {
        unsafe {
            for i in 0..(*self.scene).mNumMaterials {
                let mat = *(*self.scene).mMaterials.offset(i as isize);
                let count = assimp::aiGetMaterialTextureCount(mat, assimp::aiTextureType_DIFFUSE);
                for idx in 0..count {
                    let mut name: assimp::aiString = Default::default();
                    if assimp::aiGetMaterialTexture(
                        mat,
                        assimp::aiTextureType_DIFFUSE,
                        idx,
                        &mut name,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    ) != 0
                    {
                        panic!("Error")
                    }
                    let name = ffi::CStr::from_ptr(name.data.as_ptr());
                    let name = name.to_string_lossy();
                    let pos = name.find("assets/base/base/textures/").expect(&format!(
                        "Texture must be in the textures folder: {:?}",
                        name
                    ));

                    return name[pos + "assets/base/base/textures/".len()..name.len() - 4]
                        .to_owned();
                }
            }
            panic!("Missing texture")
        }
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        unsafe {
            assimp::aiReleaseImport(self.scene);
        }
    }
}

struct AIter<T, F, R> {
    count: isize,
    vals: *mut T,
    offset: isize,
    conv: F,
    _ret: PhantomData<R>,
}

impl<T, F, R> Iterator for AIter<T, F, R>
where
    F: Fn(*mut T) -> R,
{
    type Item = R;

    #[inline]
    fn next(&mut self) -> Option<R> {
        if self.offset < self.count {
            unsafe {
                let val = self.vals.offset(self.offset);
                self.offset += 1;
                Some((self.conv)(val))
            }
        } else {
            None
        }
    }
}

struct Mesh<'a> {
    mesh: *mut assimp::aiMesh,
    _life: PhantomData<&'a ()>,
}

impl<'a> Mesh<'a> {
    fn from(mdl: *mut *mut assimp::aiMesh) -> Mesh<'a> {
        Mesh {
            mesh: unsafe { *mdl },
            _life: PhantomData,
        }
    }

    fn name(&self) -> Cow<str> {
        unsafe {
            let name = ffi::CStr::from_ptr((*self.mesh).mName.data.as_ptr());
            name.to_string_lossy()
        }
    }

    fn mat_index(&self) -> usize {
        unsafe { (*self.mesh).mMaterialIndex as usize }
    }

    fn verts(&'a self) -> impl Iterator<Item = Vector> + 'a {
        unsafe {
            AIter {
                count: ((*self.mesh).mNumVertices) as isize,
                vals: (*self.mesh).mVertices,
                offset: 0,
                conv: to_vector,
                _ret: PhantomData,
            }
        }
    }

    fn normals(&'a self) -> impl Iterator<Item = Vector> + 'a {
        unsafe {
            AIter {
                count: ((*self.mesh).mNumVertices) as isize,
                vals: (*self.mesh).mNormals,
                offset: 0,
                conv: to_vector,
                _ret: PhantomData,
            }
        }
    }

    fn uvcoords(&'a self) -> impl Iterator<Item = Vector> + 'a {
        unsafe {
            AIter {
                count: ((*self.mesh).mNumVertices) as isize,
                vals: (*self.mesh).mTextureCoords[0],
                offset: 0,
                conv: to_vector,
                _ret: PhantomData,
            }
        }
    }

    fn faces(&'a self) -> impl Iterator<Item = model::Face> + 'a {
        unsafe {
            AIter {
                count: ((*self.mesh).mNumFaces) as isize,
                vals: (*self.mesh).mFaces,
                offset: 0,
                conv: to_face,
                _ret: PhantomData,
            }
        }
    }

    fn bones(&'a self) -> impl Iterator<Item = Bone<'a>> + 'a {
        unsafe {
            AIter {
                count: ((*self.mesh).mNumBones) as isize,
                vals: (*self.mesh).mBones,
                offset: 0,
                conv: Bone::from,
                _ret: PhantomData,
            }
        }
    }
}

struct Animation<'a> {
    ani: *mut assimp::aiAnimation,
    _life: PhantomData<&'a ()>,
}

impl<'a> Animation<'a> {
    fn from(ani: *mut *mut assimp::aiAnimation) -> Animation<'a> {
        Animation {
            ani: unsafe { *ani },
            _life: PhantomData,
        }
    }

    fn name(&self) -> Cow<str> {
        unsafe {
            let name = ffi::CStr::from_ptr((*self.ani).mName.data.as_ptr());
            name.to_string_lossy()
        }
    }

    fn channels(&'a self) -> impl Iterator<Item = NodeAnim<'a>> + 'a {
        unsafe {
            AIter {
                count: ((*self.ani).mNumChannels) as isize,
                vals: (*self.ani).mChannels,
                offset: 0,
                conv: NodeAnim::from,
                _ret: PhantomData,
            }
        }
    }
}

struct NodeAnim<'a> {
    ani: *mut assimp::aiNodeAnim,
    _life: PhantomData<&'a ()>,
}

impl<'a> NodeAnim<'a> {
    fn from(ani: *mut *mut assimp::aiNodeAnim) -> NodeAnim<'a> {
        NodeAnim {
            ani: unsafe { *ani },
            _life: PhantomData,
        }
    }

    fn name(&self) -> Cow<str> {
        unsafe {
            let name = ffi::CStr::from_ptr((*self.ani).mNodeName.data.as_ptr());
            name.to_string_lossy()
        }
    }

    fn position(&'a self) -> impl Iterator<Item = (f64, cgmath::Vector3<f32>)> + 'a {
        unsafe {
            AIter {
                count: ((*self.ani).mNumPositionKeys) as isize,
                vals: (*self.ani).mPositionKeys,
                offset: 0,
                conv: |v: *mut assimp::aiVectorKey| ((*v).mTime, mem::transmute((*v).mValue)),
                _ret: PhantomData,
            }
        }
    }

    fn rotation(&'a self) -> impl Iterator<Item = (f64, cgmath::Quaternion<f32>)> + 'a {
        unsafe {
            AIter {
                count: ((*self.ani).mNumRotationKeys) as isize,
                vals: (*self.ani).mRotationKeys,
                offset: 0,
                conv: |v: *mut assimp::aiQuatKey| ((*v).mTime, mem::transmute((*v).mValue)),
                _ret: PhantomData,
            }
        }
    }

    fn scale(&'a self) -> impl Iterator<Item = (f64, cgmath::Vector3<f32>)> + 'a {
        unsafe {
            AIter {
                count: ((*self.ani).mNumScalingKeys) as isize,
                vals: (*self.ani).mScalingKeys,
                offset: 0,
                conv: |v: *mut assimp::aiVectorKey| ((*v).mTime, mem::transmute((*v).mValue)),
                _ret: PhantomData,
            }
        }
    }
}

struct Bone<'a> {
    bone: *mut assimp::aiBone,
    _life: PhantomData<&'a ()>,
}

impl<'a> Bone<'a> {
    fn from(bone: *mut *mut assimp::aiBone) -> Bone<'a> {
        Bone {
            bone: unsafe { *bone },
            _life: PhantomData,
        }
    }

    fn name(&self) -> Cow<str> {
        unsafe {
            let name = ffi::CStr::from_ptr((*self.bone).mName.data.as_ptr());
            name.to_string_lossy()
        }
    }

    fn offset_matrix(&self) -> cgmath::Matrix4<f32> {
        unsafe {
            let mut mat = (*self.bone).mOffsetMatrix;
            assimp::aiTransposeMatrix4(&mut mat);
            mem::transmute(mat)
        }
    }

    fn weights(&self) -> impl Iterator<Item = VertexWeight> + 'a {
        unsafe {
            AIter {
                count: ((*self.bone).mNumWeights) as isize,
                vals: (*self.bone).mWeights,
                offset: 0,
                conv: VertexWeight::from,
                _ret: PhantomData,
            }
        }
    }
}

#[derive(Debug)]
struct VertexWeight {
    vertex: usize,
    weight: f32,
}

impl VertexWeight {
    fn from(vw: *mut assimp::aiVertexWeight) -> VertexWeight {
        unsafe {
            VertexWeight {
                vertex: (*vw).mVertexId as usize,
                weight: (*vw).mWeight,
            }
        }
    }
}

#[derive(Debug)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

fn to_vector(vec: *mut assimp::aiVector3D) -> Vector {
    unsafe {
        Vector {
            x: (*vec).x,
            y: (*vec).y,
            z: (*vec).z,
        }
    }
}

fn to_face(face: *mut assimp::aiFace) -> model::Face {
    unsafe {
        assert_eq!((*face).mNumIndices, 3, "Triangles required for models");
        model::Face {
            indices: [
                *(*face).mIndices.offset(0),
                *(*face).mIndices.offset(1),
                *(*face).mIndices.offset(2),
            ],
        }
    }
}

pub fn main() {
    if fs::metadata("./assets/base/base/models/").is_ok() {
        fs::remove_dir_all("./assets/base/base/models/").unwrap();
    }

    let root = Path::new("./assets-raw/models/");
    convert_all(&root, &root).unwrap()
}

fn convert_all(root: &Path, path: &Path) -> Result<(), Box<::std::error::Error>> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            convert_all(root, &path)?;
        } else {
            let path = path.strip_prefix(root).unwrap();
            if path.extension().map_or(false, |v| v == "fbx") {
                let p = path.to_string_lossy();
                let p = &p[..p.len() - 4];
                println!("Converting: {}", p);
                convert_model(p);
            }
        }
    }

    Ok(())
}

fn convert_model(name: &str) {
    let scene = Scene::import_file(&format!("./assets-raw/models/{}.fbx", name));
    let bones: usize = { scene.meshes().map(|m| m.bones().count()).sum() };
    if bones == 0 {
        convert_model_static(name, scene);
    } else {
        convert_model_animated(name, scene);
    }
}

fn convert_model_static(name: &str, scene: Scene) {
    let mut model = model::Model {
        faces: vec![],
        verts: vec![],
        texture: scene.texture(),
        transform: scene.root_transform(),
        sub_textures: vec![],
    };

    let mut vert_offset = 0;
    for mesh in scene.meshes() {
        let sub_text = scene.texture_mat(mesh.mat_index());
        model
            .sub_textures
            .push((vert_offset, sub_text.unwrap_or_else(|| scene.texture())));

        for ((v, n), uv) in mesh.verts().zip(mesh.normals()).zip(mesh.uvcoords()) {
            model.verts.push(model::Vertex {
                x: v.x,
                y: v.y,
                z: v.z,
                nx: n.x,
                ny: n.y,
                nz: n.z,
                tx: uv.x,
                ty: uv.y,
            });
        }

        for mut face in mesh.faces() {
            face.indices[0] += vert_offset as u32;
            face.indices[1] += vert_offset as u32;
            face.indices[2] += vert_offset as u32;
            model.faces.push(face);
        }
        vert_offset = model.verts.len();
    }

    let path = format!("./assets/base/base/models/{}.umod", name);
    let path = Path::new(&path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    let mut file = fs::File::create(path).unwrap();
    model.write_to(&mut file).unwrap();
}

unsafe fn build_node(node: *const assimp::aiNode) -> model::AniNode {
    let mut n = model::AniNode {
        name: {
            let name = ffi::CStr::from_ptr((*node).mName.data.as_ptr());
            name.to_string_lossy().into_owned()
        },
        transform: {
            let mut mat = (*node).mTransformation;
            assimp::aiTransposeMatrix4(&mut mat);
            mem::transmute(mat)
        },
        child_nodes: Vec::with_capacity((*node).mNumChildren as usize),
    };

    for i in 0..(*node).mNumChildren {
        n.child_nodes
            .push(build_node(*(*node).mChildren.offset(i as isize)));
    }

    n
}

fn convert_model_animated(name: &str, scene: Scene) {
    use cgmath::InnerSpace;
    let mut model = model::AniModel {
        faces: vec![],
        verts: vec![],
        texture: scene.texture(),
        transform: scene.root_transform(),
        root_node: unsafe { build_node((*scene.scene).mRootNode) },
        bones: vec![],
    };

    for ani in scene.animations() {
        let mut animation = model::Animation {
            duration: unsafe { (*ani.ani).mDuration },
            channels: HashMap::new(),
            root_node: model.root_node.clone(),
        };
        for channel in ani.channels() {
            let chan = model::AnimationDetails {
                position: channel.position().collect(),
                rotation: {
                    let mut rots: Vec<(f64, cgmath::Quaternion<f32>)> =
                        channel.rotation().collect();
                    for i in 0..rots.len() - 1 {
                        if rots[i].1.dot(rots[i + 1].1) < 0.0 {
                            rots[i + 1].1 = -rots[i + 1].1;
                        }
                    }
                    rots
                },
                scale: channel.scale().collect(),
            };
            animation.channels.insert(channel.name().into_owned(), chan);
        }
        let ani_name = ani.name();
        let ani_name = &ani_name[ani_name
            .char_indices()
            .find(|v| v.1 == '|')
            .map_or(0, |v| v.0)
            + 1..];

        let path = format!("./assets/base/base/models/{}_{}.uani", name, ani_name);
        let path = Path::new(&path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        let mut file = fs::File::create(path).unwrap();
        animation.write_to(&mut file).unwrap();
    }

    let mut vert_offset = 0;
    for mesh in scene.meshes() {
        for ((v, n), uv) in mesh.verts().zip(mesh.normals()).zip(mesh.uvcoords()) {
            model.verts.push(model::AniVertex {
                x: v.x,
                y: v.y,
                z: v.z,
                nx: n.x,
                ny: n.y,
                nz: n.z,
                tx: uv.x,
                ty: uv.y,
                bones: [0; 4],
                bone_weights: [1.0, 0.0, 0.0, 0.0],
            });
        }

        for mut face in mesh.faces() {
            face.indices[0] += vert_offset as u32;
            face.indices[1] += vert_offset as u32;
            face.indices[2] += vert_offset as u32;
            model.faces.push(face);
        }

        for bone in mesh.bones() {
            let bone_id = model.bones.len();
            model.bones.push(model::AniBone {
                name: bone.name().into_owned(),
                offset: bone.offset_matrix(),
            });
            for vw in bone.weights() {
                let vert = &mut model.verts[vw.vertex + vert_offset];
                for (b, bw) in vert.bones.iter_mut().zip(&mut vert.bone_weights) {
                    if *b == 0 {
                        *b = bone_id as u8 + 1;
                        *bw = vw.weight;
                        break;
                    }
                }
            }
        }

        vert_offset = model.verts.len();
    }

    let path = format!("./assets/base/base/models/{}.uamod", name);
    let path = Path::new(&path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    let mut file = fs::File::create(path).unwrap();
    model.write_to(&mut file).unwrap();
}
