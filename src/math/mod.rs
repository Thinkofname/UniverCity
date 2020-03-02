//! General math functions not from a library

use crate::prelude::*;
use cgmath::prelude::*;

/// A viewing frustum
#[derive(Clone)]
pub struct Frustum {
    right: Plane,
    left: Plane,
    bottom: Plane,
    top: Plane,
    far: Plane,
    near: Plane,
}

impl Frustum {
    /// Creates a frustum from the passed matrix
    pub fn from_matrix(mat: cgmath::Matrix4<f32>) -> Frustum {
        Frustum {
            left: (mat.row(3) + mat.row(0)).normalize().into(),
            right: (mat.row(3) - mat.row(0)).normalize().into(),
            bottom: (mat.row(3) + mat.row(1)).normalize().into(),
            top: (mat.row(3) - mat.row(1)).normalize().into(),
            near: (mat.row(3) + mat.row(2)).normalize().into(),
            far: (mat.row(3) - mat.row(2)).normalize().into(),
        }
    }

    /// Returns whether the point is within the frustum
    pub fn contains_point(&self, p: cgmath::Vector3<f32>) -> bool {
        for plane in &[
            self.right,
            self.left,
            self.bottom,
            self.top,
            self.far,
            self.near,
        ] {
            if plane.normal.dot(p) - plane.distance < 0.0 {
                return false;
            }
        }
        true
    }

    /// Returns whether the sphere is within the frustum
    pub fn contains_sphere(&self, p: cgmath::Vector3<f32>, radius: f32) -> bool {
        for plane in &[
            self.right,
            self.left,
            self.bottom,
            self.top,
            self.far,
            self.near,
        ] {
            if plane.normal.dot(p) - plane.distance < -radius {
                return false;
            }
        }
        true
    }

    /// Returns whether the AABB as represented by the
    /// two points (min, max) is within the frustum.
    pub fn contains_aabb(&self, min: cgmath::Vector3<f32>, max: cgmath::Vector3<f32>) -> bool {
        for plane in &[
            self.right,
            self.left,
            self.bottom,
            self.top,
            self.far,
            self.near,
        ] {
            let bx = if plane.normal.x < 0.0 { min.x } else { max.x };
            let by = if plane.normal.y < 0.0 { min.y } else { max.y };
            let bz = if plane.normal.z < 0.0 { min.z } else { max.z };
            let distance = plane.normal.dot(cgmath::Vector3::new(bx, by, bz));
            if distance < plane.distance {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Copy)]
struct Plane {
    normal: cgmath::Vector3<f32>,
    distance: f32,
}

impl From<cgmath::Vector4<f32>> for Plane {
    fn from(v: cgmath::Vector4<f32>) -> Plane {
        Plane {
            normal: v.truncate(),
            distance: -v.w,
        }
    }
}
