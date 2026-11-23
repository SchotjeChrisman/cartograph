use crate::util;
use std::fmt::Display;

/// Anything with an area.
pub trait Shape: Named {
    fn area(&self) -> f64;

    fn doubled(&self) -> f64 {
        self.area() * 2.0
    }
}

pub trait Named {}

pub struct Circle {
    pub radius: f64,
}

pub struct Square {
    pub side: f64,
}

pub enum Kind {
    Round(f64),
    Flat,
}

impl Named for Circle {}

impl Circle {
    pub fn new(radius: f64) -> Self {
        util::helper();
        Self { radius }
    }

    /// Both Circle and Square have `describe`, so a call on an unknown receiver is ambiguous.
    pub fn describe(&self) -> String {
        format!("circle {}", self.radius)
    }
}

impl Square {
    pub fn describe(&self) -> String {
        String::new()
    }

    fn only_here(&self) -> f64 {
        self.side
    }
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        let _ = Kind::Round(self.radius);
        3.14 * self.radius * self.radius
    }
}

impl Display for Square {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.only_here())
    }
}

pub fn unit_area() -> f64 {
    Circle::new(1.0).area()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit() {
        assert!(unit_area() > 3.0);
        helper_in_tests();
    }

    fn helper_in_tests() {}
}
