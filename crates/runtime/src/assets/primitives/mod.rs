pub(super) mod capsule;
pub(super) mod cube;
pub(super) mod plane;
pub(super) mod polygon;
mod rounded;
pub(super) mod sphere;
pub(super) mod triangle;

#[cfg(test)]
#[path = "../../../tests/unit/assets/primitives.rs"]
mod tests;
