//! Geometric shapes used to exercise type and signature changes.

use crate::math::clamp01;

/// A point in the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// Origin of the coordinate system.
    pub const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

    /// Euclidean norm of the point.
    pub fn norm(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Returns a translated copy.
    pub fn translated(&self, dx: f64, dy: f64) -> Point {
        Point {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    /// Unit vector, or the origin when the norm is zero.
    pub fn normalized(&self) -> Point {
        let n = self.norm();
        if n == 0.0 {
            Point::ORIGIN
        } else {
            Point {
                x: clamp01(self.x / n),
                y: clamp01(self.y / n),
            }
        }
    }
}

/// Supported shape kinds.
#[derive(Debug, Clone, Copy)]
pub enum Shape {
    /// Axis-aligned rectangle.
    Rect {
        origin: Point,
        width: f64,
        height: f64,
    },
    /// Circle.
    Circle { center: Point, radius: f64 },
}

impl Shape {
    /// Surface area of the shape.
    pub fn area(&self) -> f64 {
        match *self {
            Shape::Rect { width, height, .. } => width * height,
            Shape::Circle { radius, .. } => core::f64::consts::PI * radius * radius,
        }
    }

    /// Center of mass of the shape.
    pub fn centroid(&self) -> Point {
        match *self {
            Shape::Rect {
                origin,
                width,
                height,
            } => origin.translated(width / 2.0, height / 2.0),
            Shape::Circle { center, .. } => center,
        }
    }

    /// Axis-aligned bounding box of the shape, as `(min, max)` corners.
    pub fn bounds(&self) -> (Point, Point) {
        match *self {
            Shape::Rect {
                origin,
                width,
                height,
            } => (
                origin,
                origin.translated(width.abs(), height.abs()),
            ),
            Shape::Circle { center, radius } => (
                center.translated(-radius.abs(), -radius.abs()),
                center.translated(radius.abs(), radius.abs()),
            ),
        }
    }
}
