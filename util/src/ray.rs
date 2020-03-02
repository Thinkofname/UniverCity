use cgmath::Vector3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ray<F = f32> {
    pub start: Vector3<F>,
    pub direction: Vector3<F>,
}
