use super::Ray;
use cgmath::{BaseFloat, Vector3};

#[derive(Debug, Clone, Copy, PartialEq, Eq, DeltaEncode)]
pub struct AABB<F = f32> {
    pub min: Vector3<F>,
    pub max: Vector3<F>,
}

impl<F: BaseFloat> AABB<F> {
    pub fn contains(self, v: Vector3<F>) -> bool {
        !(v.x < self.min.x
            || v.x > self.max.x
            || v.y < self.min.y
            || v.y > self.max.y
            || v.z < self.min.z
            || v.z > self.max.z)
    }

    pub fn intersects_ray(self, ray: Ray<F>) -> bool {
        use cgmath::{Array, ElementWise};
        let dir_frac = Vector3::from_value(F::one()).div_element_wise(ray.direction);

        let mut tmin = F::neg_infinity();
        let mut tmax = F::infinity();

        // These are vectors not vecs, its harder to
        // iterate over them
        #[allow(clippy::needless_range_loop)]
        for i in 0..3 {
            let t1 = (self.min[i] - ray.start[i]) * dir_frac[i];
            let t2 = (self.max[i] - ray.start[i]) * dir_frac[i];

            tmin = tmin.max(t1.min(t2));
            tmax = tmax.min(t1.max(t2));
        }

        !(tmax < F::zero() || tmin > tmax)
    }
}
