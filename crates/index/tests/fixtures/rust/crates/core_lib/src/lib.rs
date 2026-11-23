//! Shapes and helpers.

pub mod shapes;
pub mod util;

mod gone;

pub fn describe_all() -> usize {
    util::helper() + shapes::unit_area() as usize
}
