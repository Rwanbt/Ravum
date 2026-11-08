//! Edit chains for the incremental-analysis campaign.
//!
//! Each category is a sequence of cumulative edits applied to the same fixture
//! unit. Every edit is a literal `find` → `replace` operation that must match
//! **exactly once**; a missing or ambiguous anchor aborts the campaign instead
//! of silently measuring a different edit.
//!
//! The categories model real editing sessions, one commit-like step at a time:
//!
//! * `body` — local function bodies, no public API change;
//! * `signature` — public signatures change and the callers in another module
//!   are updated accordingly (multi-file edit);
//! * `type` — struct/enum/type changes that force callers to adapt;
//! * `constant` — constant values and new constants;
//! * `import` — import/re-export rewrites;
//! * `identical` — the same buffer submitted again (cache-hit path);
//! * `invalid_repair` — an incomplete buffer, then the repaired buffer.

use ravum_daemon::SourceFile;

/// One literal replacement in one fixture file.
#[derive(Debug, Clone, Copy)]
pub struct Edit {
    pub file: &'static str,
    pub find: &'static str,
    pub replace: &'static str,
}

/// What kind of edit sequence a category represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryKind {
    /// Each step is a real edit from the previous state.
    EditChain,
    /// The same content is submitted repeatedly.
    Identical,
    /// Each sample is `(incomplete buffer, repaired buffer)`.
    InvalidRepair,
}

pub struct CategorySpec {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: CategoryKind,
    /// One entry per sample: the edits that turn the previous state into this
    /// sample's state.
    pub steps: Vec<Vec<Edit>>,
}

const BODY: &[&[Edit]] = &[
    &[Edit {
        file: "src/math.rs",
        find: "x * SCALE",
        replace: "x * SCALE + 0.5",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "total += *value;",
        replace: "total += *value - 0.25;",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "if x < 0.0 {",
        replace: "if x < -0.0 {",
    }],
    &[Edit {
        file: "src/shapes.rs",
        find: "(self.x * self.x + self.y * self.y).sqrt()",
        replace: "(self.x * self.x + self.y * self.y + 1.0).sqrt()",
    }],
    &[Edit {
        file: "src/pipeline.rs",
        find: "total / values.len() as f64",
        replace: "(total / values.len() as f64) * 0.99",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "values.iter().copied().map(|value| op(value)).collect()",
        replace: "values.iter().copied().map(|value| op(value) + 0.0).collect()",
    }],
    &[Edit {
        file: "src/shapes.rs",
        find: "=> width * height,",
        replace: "=> width * height * 1.0,",
    }],
    &[Edit {
        file: "src/pipeline.rs",
        find: r#"format!("{:?} with area {:.3}", shape, shape.area())"#,
        replace: r#"format!("{:?} area={:.4}", shape, shape.area())"#,
    }],
    &[Edit {
        file: "src/math.rs",
        find: "- 0.25;",
        replace: "- 0.5;",
    }],
    &[Edit {
        file: "src/shapes.rs",
        find: "if n == 0.0 {",
        replace: "if n <= f64::EPSILON {",
    }],
];

const SIGNATURE: &[&[Edit]] = &[
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn scale(x: f64) -> f64 {",
            replace: "pub fn scale_pair(x: f64, factor: f64) -> f64 {",
        },
        Edit {
            file: "src/math.rs",
            find: "    x * SCALE\n}",
            replace: "    x * factor\n}",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "use crate::math::{accumulate, clamp01, histogram, map_all, scale, SCALE};",
            replace: "use crate::math::{accumulate, clamp01, histogram, map_all, scale_pair, SCALE};",
        },
        Edit {
            file: "src/lib.rs",
            find: "pub use math::{accumulate, clamp01, histogram, map_all, scale, SCALE};",
            replace: "pub use math::{accumulate, clamp01, histogram, map_all, scale_pair, SCALE};",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "map_all(values, |value| scale(clamp01(value)))",
            replace: "map_all(values, |value| scale_pair(clamp01(value), SCALE))",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn scale_pair(x: f64, factor: f64) -> f64 {",
            replace: "pub fn scale_pair(x: f64, factor: f64, offset: f64) -> f64 {",
        },
        Edit {
            file: "src/math.rs",
            find: "    x * factor\n}",
            replace: "    x * factor + offset\n}",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "scale_pair(clamp01(value), SCALE)",
            replace: "scale_pair(clamp01(value), SCALE, 0.0)",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn scale_pair(x: f64, factor: f64, offset: f64) -> f64 {",
            replace: "pub fn apply_scale(x: f64, factor: f64, offset: f64) -> f64 {",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "scale_pair,",
            replace: "apply_scale,",
        },
        Edit {
            file: "src/lib.rs",
            find: "scale_pair,",
            replace: "apply_scale,",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "scale_pair(clamp01(value), SCALE, 0.0)",
            replace: "apply_scale(clamp01(value), SCALE, 0.0)",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn apply_scale(x: f64, factor: f64, offset: f64) -> f64 {",
            replace: "pub fn apply_scale(factor: f64, x: f64, offset: f64) -> f64 {",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "apply_scale(clamp01(value), SCALE, 0.0)",
            replace: "apply_scale(SCALE, clamp01(value), 0.0)",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn apply_scale(factor: f64, x: f64, offset: f64) -> f64 {",
            replace: "pub fn apply_scale(factor: f64, x: f64, offset: f64) -> f32 {",
        },
        Edit {
            file: "src/math.rs",
            find: "    x * factor + offset\n}",
            replace: "    (x * factor + offset) as f32\n}",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "apply_scale(SCALE, clamp01(value), 0.0)",
            replace: "apply_scale(SCALE, clamp01(value), 0.0) as f64",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn apply_scale(factor: f64, x: f64, offset: f64) -> f32 {",
            replace: "pub fn apply_scale(factor: f64, x: f64) -> f32 {",
        },
        Edit {
            file: "src/math.rs",
            find: "    (x * factor + offset) as f32",
            replace: "    (x * factor) as f32",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "apply_scale(SCALE, clamp01(value), 0.0) as f64",
            replace: "apply_scale(SCALE, clamp01(value)) as f64",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn apply_scale(factor: f64, x: f64) -> f32 {",
            replace: "pub fn scaled_with(factor: f64, x: f64) -> f32 {",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "apply_scale,",
            replace: "scaled_with,",
        },
        Edit {
            file: "src/lib.rs",
            find: "apply_scale,",
            replace: "scaled_with,",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "apply_scale(SCALE, clamp01(value)) as f64",
            replace: "scaled_with(SCALE, clamp01(value)) as f64",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn scaled_with(factor: f64, x: f64) -> f32 {",
            replace: "pub fn scaled_with(factor: &f64, x: f64) -> f32 {",
        },
        Edit {
            file: "src/math.rs",
            find: "    (x * factor) as f32",
            replace: "    (x * *factor) as f32",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "scaled_with(SCALE, clamp01(value)) as f64",
            replace: "scaled_with(&SCALE, clamp01(value)) as f64",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn scaled_with(factor: &f64, x: f64) -> f32 {",
            replace: "pub fn scaled_with(factor: &f64, x: f64) -> f64 {",
        },
        Edit {
            file: "src/math.rs",
            find: "    (x * *factor) as f32",
            replace: "    x * *factor",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "scaled_with(&SCALE, clamp01(value)) as f64",
            replace: "scaled_with(&SCALE, clamp01(value))",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn scaled_with(factor: &f64, x: f64) -> f64 {",
            replace: "pub fn scaled_with(factor: &f64, x: f64, bias: f64) -> f64 {",
        },
        Edit {
            file: "src/math.rs",
            find: "    x * *factor\n}",
            replace: "    x * *factor + bias\n}",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "scaled_with(&SCALE, clamp01(value))",
            replace: "scaled_with(&SCALE, clamp01(value), 0.0)",
        },
    ],
];

const TYPE_CHANGE: &[&[Edit]] = &[
    &[
        Edit {
            file: "src/shapes.rs",
            find: "pub struct Point {\n    pub x: f64,\n    pub y: f64,\n}",
            replace: "pub struct Point {\n    pub x: f64,\n    pub y: f64,\n    pub z: f64,\n}",
        },
        Edit {
            file: "src/shapes.rs",
            find: "pub const ORIGIN: Point = Point { x: 0.0, y: 0.0 };",
            replace: "pub const ORIGIN: Point = Point { x: 0.0, y: 0.0, z: 0.0 };",
        },
        Edit {
            file: "src/shapes.rs",
            find: "        Point {\n            x: self.x + dx,\n            y: self.y + dy,\n        }",
            replace: "        Point {\n            x: self.x + dx,\n            y: self.y + dy,\n            z: self.z + 0.0,\n        }",
        },
        Edit {
            file: "src/shapes.rs",
            find: "            Point {\n                x: clamp01(self.x / n),\n                y: clamp01(self.y / n),\n            }",
            replace: "            Point {\n                x: clamp01(self.x / n),\n                y: clamp01(self.y / n),\n                z: clamp01(self.z / n),\n            }",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "let Point { x, y } = shape.centroid();",
            replace: "let Point { x, y, .. } = shape.centroid();",
        },
    ],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "    /// Circle.\n    Circle { center: Point, radius: f64 },",
            replace: "    /// Circle.\n    Circle { center: Point, radius: f32 },",
        },
        Edit {
            file: "src/shapes.rs",
            find: "Shape::Circle { radius, .. } => core::f64::consts::PI * radius * radius,",
            replace: "Shape::Circle { radius, .. } => core::f64::consts::PI * (radius as f64) * (radius as f64),",
        },
        Edit {
            file: "src/shapes.rs",
            find: "center.translated(-radius.abs(), -radius.abs()),",
            replace: "center.translated(-(radius as f64).abs(), -(radius as f64).abs()),",
        },
        Edit {
            file: "src/shapes.rs",
            find: "center.translated(radius.abs(), radius.abs()),",
            replace: "center.translated((radius as f64).abs(), (radius as f64).abs()),",
        },
    ],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "    /// Circle.\n    Circle { center: Point, radius: f32 },\n}",
            replace: "    /// Circle.\n    Circle { center: Point, radius: f32 },\n    /// Triangle.\n    Triangle { a: Point, b: Point, c: Point },\n}",
        },
        Edit {
            file: "src/shapes.rs",
            find: "Shape::Circle { radius, .. } => core::f64::consts::PI * (radius as f64) * (radius as f64),\n        }",
            replace: "Shape::Circle { radius, .. } => core::f64::consts::PI * (radius as f64) * (radius as f64),\n            Shape::Triangle { a, b, c } => {\n                let ab = a.translated(-b.x, -b.y).norm();\n                let bc = b.translated(-c.x, -c.y).norm();\n                let ca = c.translated(-a.x, -a.y).norm();\n                let s = (ab + bc + ca) / 2.0;\n                (s * (s - ab) * (s - bc) * (s - ca)).max(0.0).sqrt()\n            }\n        }",
        },
        Edit {
            file: "src/shapes.rs",
            find: "Shape::Circle { center, .. } => center,\n        }",
            replace: "Shape::Circle { center, .. } => center,\n            Shape::Triangle { a, b, c } => Point {\n                x: (a.x + b.x + c.x) / 3.0,\n                y: (a.y + b.y + c.y) / 3.0,\n                z: (a.z + b.z + c.z) / 3.0,\n            },\n        }",
        },
        Edit {
            file: "src/shapes.rs",
            find: "            Shape::Circle { center, radius } => (\n                center.translated(-(radius as f64).abs(), -(radius as f64).abs()),\n                center.translated((radius as f64).abs(), (radius as f64).abs()),\n            ),\n        }",
            replace: "            Shape::Circle { center, radius } => (\n                center.translated(-(radius as f64).abs(), -(radius as f64).abs()),\n                center.translated((radius as f64).abs(), (radius as f64).abs()),\n            ),\n            Shape::Triangle { a, b, c } => (\n                Point {\n                    x: a.x.min(b.x).min(c.x),\n                    y: a.y.min(b.y).min(c.y),\n                    z: a.z.min(b.z).min(c.z),\n                },\n                Point {\n                    x: a.x.max(b.x).max(c.x),\n                    y: a.y.max(b.y).max(c.y),\n                    z: a.z.max(b.z).max(c.z),\n                },\n            ),\n        }",
        },
    ],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "    /// Triangle.\n    Triangle { a: Point, b: Point, c: Point },\n",
            replace: "",
        },
        Edit {
            file: "src/shapes.rs",
            find: "            Shape::Triangle { a, b, c } => {\n                let ab = a.translated(-b.x, -b.y).norm();\n                let bc = b.translated(-c.x, -c.y).norm();\n                let ca = c.translated(-a.x, -a.y).norm();\n                let s = (ab + bc + ca) / 2.0;\n                (s * (s - ab) * (s - bc) * (s - ca)).max(0.0).sqrt()\n            }\n",
            replace: "",
        },
        Edit {
            file: "src/shapes.rs",
            find: "            Shape::Triangle { a, b, c } => Point {\n                x: (a.x + b.x + c.x) / 3.0,\n                y: (a.y + b.y + c.y) / 3.0,\n                z: (a.z + b.z + c.z) / 3.0,\n            },\n",
            replace: "",
        },
        Edit {
            file: "src/shapes.rs",
            find: "            Shape::Triangle { a, b, c } => (\n                Point {\n                    x: a.x.min(b.x).min(c.x),\n                    y: a.y.min(b.y).min(c.y),\n                    z: a.z.min(b.z).min(c.z),\n                },\n                Point {\n                    x: a.x.max(b.x).max(c.x),\n                    y: a.y.max(b.y).max(c.y),\n                    z: a.z.max(b.z).max(c.z),\n                },\n            ),\n",
            replace: "",
        },
    ],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "    Rect {\n        origin: Point,\n        width: f64,\n        height: f64,\n    },",
            replace: "    Rect {\n        corner: Point,\n        w: f64,\n        h: f64,\n    },",
        },
        Edit {
            file: "src/shapes.rs",
            find: "Shape::Rect { width, height, .. } => width * height,",
            replace: "Shape::Rect { w, h, .. } => w * h,",
        },
        Edit {
            file: "src/shapes.rs",
            find: "            Shape::Rect {\n                origin,\n                width,\n                height,\n            } => origin.translated(width / 2.0, height / 2.0),",
            replace: "            Shape::Rect {\n                corner,\n                w,\n                h,\n            } => corner.translated(w / 2.0, h / 2.0),",
        },
        Edit {
            file: "src/shapes.rs",
            find: "            Shape::Rect {\n                origin,\n                width,\n                height,\n            } => (\n                origin,\n                origin.translated(width.abs(), height.abs()),\n            ),",
            replace: "            Shape::Rect {\n                corner,\n                w,\n                h,\n            } => (\n                corner,\n                corner.translated(w.abs(), h.abs()),\n            ),",
        },
    ],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "    pub const ORIGIN: Point = Point { x: 0.0, y: 0.0, z: 0.0 };",
            replace: "    pub fn origin() -> Point {\n        Point { x: 0.0, y: 0.0, z: 0.0 }\n    }",
        },
        Edit {
            file: "src/shapes.rs",
            find: "Point::ORIGIN",
            replace: "Point::origin()",
        },
    ],
    &[Edit {
        file: "src/shapes.rs",
        find: "}\n\n/// Supported shape kinds.",
        replace: "\n    /// Length of the segment between this point and `other`.\n    pub fn distance_to(&self, other: Point) -> f64 {\n        self.translated(-other.x, -other.y).norm()\n    }\n}\n\n/// A directed segment between two points.\n#[derive(Debug, Clone, Copy)]\npub struct Segment {\n    pub start: Point,\n    pub end: Point,\n}\n\nimpl Segment {\n    /// Euclidean length of the segment.\n    pub fn length(&self) -> f64 {\n        self.start.distance_to(self.end)\n    }\n}\n\n/// Supported shape kinds.",
    }],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn histogram(values: &[f64]) -> [usize; BUCKETS] {",
            replace: "pub fn histogram(values: &[f64]) -> Vec<usize> {",
        },
        Edit {
            file: "src/math.rs",
            find: "    let mut buckets = [0usize; BUCKETS];",
            replace: "    let mut buckets = vec![0usize; BUCKETS];",
        },
    ],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "pub fn norm(&self) -> f64",
            replace: "pub fn norm(self) -> f64",
        },
        Edit {
            file: "src/shapes.rs",
            find: "pub fn translated(&self, dx: f64, dy: f64) -> Point",
            replace: "pub fn translated(self, dx: f64, dy: f64) -> Point",
        },
        Edit {
            file: "src/shapes.rs",
            find: "pub fn normalized(&self) -> Point",
            replace: "pub fn normalized(self) -> Point",
        },
    ],
    &[
        Edit {
            file: "src/math.rs",
            find: "pub fn accumulate(values: &[f64]) -> f64 {",
            replace: "pub fn accumulate(values: impl AsRef<[f64]>) -> f64 {\n    let values = values.as_ref();",
        },
    ],
];

const CONSTANT: &[&[Edit]] = &[
    &[Edit {
        file: "src/math.rs",
        find: "pub const SCALE: f64 = 2.5;",
        replace: "pub const SCALE: f64 = 2.75;",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "pub const SCALE: f64 = 2.75;",
        replace: "pub const SCALE: f64 = 3.25;",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "pub const SCALE: f64 = 3.25;\n",
        replace: "pub const SCALE: f64 = 3.25;\n\n/// Tolerance used by the fixture helpers.\npub const EPSILON: f64 = 1e-9;\n",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "pub const BUCKETS: usize = 8;",
        replace: "pub const BUCKETS: usize = 16;",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "pub const BUCKETS: usize = 16;",
        replace: "pub const BUCKETS: usize = 32;",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "pub const BUCKETS: usize = 32;\n",
        replace: "pub const BUCKETS: usize = 32;\n\n/// Canonical sample corners.\npub const DEFAULT_SAMPLES: [f64; 4] = [0.0, 0.25, 0.5, 1.0];\n",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "[0.0, 0.25, 0.5, 1.0]",
        replace: "[0.1, 0.2, 0.3, 0.4]",
    }],
    &[Edit {
        file: "src/math.rs",
        find: "pub const EPSILON: f64 = 1e-9;",
        replace: "pub const EPSILON: f64 = 1e-12;",
    }],
    &[Edit {
        file: "src/shapes.rs",
        find: "use crate::math::clamp01;\n",
        replace: "use crate::math::clamp01;\n\n/// Default radius for the circle examples.\npub const DEFAULT_RADIUS: f32 = 1.0;\n",
    }],
    &[
        Edit {
            file: "src/pipeline.rs",
            find: "use crate::shapes::{Point, Shape};\n\n",
            replace: "use crate::shapes::{Point, Shape};\n\n/// Bias applied to every score.\nconst SCORE_OFFSET: f64 = 0.125;\n\n",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "    total / values.len() as f64\n}",
            replace: "    total / values.len() as f64 + SCORE_OFFSET\n}",
        },
    ],
];

const IMPORT: &[&[Edit]] = &[
    &[Edit {
        file: "src/pipeline.rs",
        find: "use crate::math::{accumulate, clamp01, histogram, map_all, scale, SCALE};",
        replace: "use crate::math::accumulate;\nuse crate::math::{clamp01, histogram, map_all, scale, SCALE};",
    }],
    &[
        Edit {
            file: "src/pipeline.rs",
            find: "use crate::math::{clamp01, histogram, map_all, scale, SCALE};",
            replace: "use crate::math::{clamp01, histogram, map_all, scale as scale_fn, SCALE};",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "scale(clamp01(value))",
            replace: "scale_fn(clamp01(value))",
        },
    ],
    &[Edit {
        file: "src/pipeline.rs",
        find: "use crate::shapes::{Point, Shape};",
        replace: "use crate::shapes::*;",
    }],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "use crate::math::clamp01;\n",
            replace: "use crate::math::clamp01;\nuse core::f64::consts::PI;\n",
        },
        Edit {
            file: "src/shapes.rs",
            find: "=> core::f64::consts::PI * radius * radius,",
            replace: "=> PI * radius * radius,",
        },
    ],
    &[
        Edit {
            file: "src/shapes.rs",
            find: "use crate::math::clamp01;\nuse core::f64::consts::PI;\n",
            replace: "use crate::math::clamp01;\n",
        },
        Edit {
            file: "src/shapes.rs",
            find: "=> PI * radius * radius,",
            replace: "=> core::f64::consts::PI * radius * radius,",
        },
    ],
    &[Edit {
        file: "src/pipeline.rs",
        find: "use crate::math::{clamp01, histogram, map_all, scale as scale_fn, SCALE};",
        replace: "use crate::math::{SCALE, clamp01, histogram, map_all, scale as scale_fn};",
    }],
    &[Edit {
        file: "src/lib.rs",
        find: "pub use math::{accumulate, clamp01, histogram, map_all, scale, SCALE};",
        replace: "pub use math::SCALE;\npub use math::{accumulate, clamp01, histogram, map_all, scale};",
    }],
    &[Edit {
        file: "src/lib.rs",
        find: "pub use shapes::{Point, Shape};",
        replace: "pub use shapes::Point;\npub use shapes::Shape;",
    }],
    &[
        Edit {
            file: "src/pipeline.rs",
            find: "use crate::math::{SCALE, clamp01, histogram, map_all, scale as scale_fn};",
            replace: "use crate::math::{SCALE, clamp01, histogram as bucketize, map_all, scale as scale_fn};",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "let buckets = histogram(values);",
            replace: "let buckets = bucketize(values);",
        },
    ],
    &[
        Edit {
            file: "src/pipeline.rs",
            find: "use crate::math::accumulate;",
            replace: "use crate::math as math_mod;",
        },
        Edit {
            file: "src/pipeline.rs",
            find: "let total = accumulate(&scaled);",
            replace: "let total = math_mod::accumulate(&scaled);",
        },
    ],
];

/// All measurement categories, in campaign order.
pub fn categories() -> Vec<CategorySpec> {
    vec![
        CategorySpec {
            name: "identical",
            description: "same buffer submitted again: cache-hit path (MVP-0a artifact cache analogue)",
            kind: CategoryKind::Identical,
            steps: Vec::new(),
        },
        CategorySpec {
            name: "body",
            description: "local function body edits, no public API change",
            kind: CategoryKind::EditChain,
            steps: BODY.iter().map(|step| step.to_vec()).collect(),
        },
        CategorySpec {
            name: "signature",
            description: "public signature changes with callers updated in another module",
            kind: CategoryKind::EditChain,
            steps: SIGNATURE.iter().map(|step| step.to_vec()).collect(),
        },
        CategorySpec {
            name: "type",
            description: "struct/enum/type changes forcing callers to adapt",
            kind: CategoryKind::EditChain,
            steps: TYPE_CHANGE.iter().map(|step| step.to_vec()).collect(),
        },
        CategorySpec {
            name: "constant",
            description: "constant value and constant definition edits",
            kind: CategoryKind::EditChain,
            steps: CONSTANT.iter().map(|step| step.to_vec()).collect(),
        },
        CategorySpec {
            name: "import",
            description: "import and re-export rewrites",
            kind: CategoryKind::EditChain,
            steps: IMPORT.iter().map(|step| step.to_vec()).collect(),
        },
        CategorySpec {
            name: "invalid_repair",
            description: "incomplete buffer (syntax error) then the repaired buffer",
            kind: CategoryKind::InvalidRepair,
            steps: Vec::new(),
        },
    ]
}

/// Applies one edit to the current file set. Fails loudly when the anchor is
/// missing or ambiguous: a measurement of *the wrong edit* is worthless.
pub fn apply_edit(files: &mut [SourceFile], edit: &Edit) -> Result<(), String> {
    let file = files
        .iter_mut()
        .find(|file| file.path == edit.file)
        .ok_or_else(|| format!("edit targets unknown file {}", edit.file))?;
    let occurrences = file.content.matches(edit.find).count();
    if occurrences != 1 {
        return Err(format!(
            "edit anchor in {} matched {occurrences} times (expected exactly 1): {:?}",
            edit.file, edit.find
        ));
    }
    file.content = file.content.replacen(edit.find, edit.replace, 1);
    Ok(())
}

/// Applies one cumulative step (possibly several files) to the current state.
pub fn apply_step(files: &mut [SourceFile], step: &[Edit]) -> Result<(), String> {
    for edit in step {
        apply_edit(files, edit)?;
    }
    Ok(())
}

/// Builds the recorded states for a category.
///
/// Returns `(kind, states)` where `states[i]` is the content submitted for
/// sample `i + 1`. For `InvalidRepair`, each sample has two states: the
/// incomplete buffer, then the repaired buffer.
pub fn build_states(
    base: &[SourceFile],
    samples: usize,
    category: &CategorySpec,
) -> Result<Vec<Vec<SourceFile>>, String> {
    let mut states = Vec::new();
    match category.kind {
        CategoryKind::Identical => {
            for _ in 0..samples {
                states.push(base.to_vec());
            }
        }
        CategoryKind::EditChain => {
            let mut current = base.to_vec();
            for (index, step) in category.steps.iter().enumerate().take(samples) {
                apply_step(&mut current, step)
                    .map_err(|error| format!("{} step {}: {error}", category.name, index + 1))?;
                states.push(current.clone());
            }
            if states.len() < samples {
                return Err(format!(
                    "{} has only {} edit steps for {samples} samples",
                    category.name,
                    states.len()
                ));
            }
        }
        CategoryKind::InvalidRepair => {
            for sample in 1..=samples {
                // A distinct truncation per sample models an editor buffer being
                // incomplete at a different point of the typing session.
                let mut broken = base.to_vec();
                let target = broken
                    .iter_mut()
                    .find(|file| file.path == "src/math.rs")
                    .ok_or_else(|| "fixture has no src/math.rs".to_owned())?;
                let keep = target
                    .content
                    .len()
                    .saturating_sub(8 + sample * 11)
                    .max(1);
                let keep = floor_char_boundary(&target.content, keep);
                target.content.truncate(keep);
                states.push(broken);
                states.push(base.to_vec());
            }
        }
    }
    Ok(states)
}

/// Largest index below `at` that is a UTF-8 char boundary.
fn floor_char_boundary(text: &str, at: usize) -> usize {
    let mut index = at.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_files() -> Vec<SourceFile> {
        // Same fixture the campaign uses, read from the crate manifest directory.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/unit/src");
        ["lib.rs", "math.rs", "pipeline.rs", "shapes.rs"]
            .iter()
            .map(|name| SourceFile {
                path: format!("src/{name}"),
                content: std::fs::read_to_string(dir.join(name)).unwrap(),
            })
            .collect()
    }

    #[test]
    fn every_edit_chain_applies_cleanly() {
        let base = base_files();
        for category in categories() {
            let samples = if category.kind == CategoryKind::EditChain {
                category.steps.len()
            } else {
                10
            };
            let states =
                build_states(&base, samples, &category).unwrap_or_else(|error| panic!("{error}"));
            assert!(!states.is_empty());
            if category.kind == CategoryKind::EditChain {
                assert_eq!(states.len(), category.steps.len());
                // Every step must actually change the content.
                let mut previous = base.clone();
                for (index, state) in states.iter().enumerate() {
                    assert_ne!(
                        previous, *state,
                        "{} step {} produced identical content",
                        category.name,
                        index + 1
                    );
                    previous = state.clone();
                }
            }
        }
    }

    #[test]
    fn invalid_repair_pairs_have_distinct_truncations() {
        let base = base_files();
        let category = categories()
            .into_iter()
            .find(|category| category.kind == CategoryKind::InvalidRepair)
            .unwrap();
        let states = build_states(&base, 3, &category).unwrap();
        assert_eq!(states.len(), 6);
        let broken_a = &states[0];
        let broken_b = &states[2];
        assert_ne!(broken_a, broken_b, "each sample must break differently");
        assert_eq!(states[1], base, "repair restores the known-good buffer");
        assert_eq!(states[3], base);
    }

    #[test]
    fn missing_anchor_is_a_hard_error() {
        let mut files = base_files();
        let bad = Edit {
            file: "src/math.rs",
            find: "this string is not in the fixture",
            replace: "x",
        };
        assert!(apply_edit(&mut files, &bad).is_err());
    }
}
