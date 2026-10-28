//! HobbyCAM 2D geometry core: primitives, transforms, contours, chaining and containment.
//! Units are millimetres, angles radians CCW from +X, Y axis up.

mod bbox;
mod chain;
mod contour;
pub mod fillet;
mod point;
mod segment;
pub mod shapes;
mod transform;
mod tree;

pub use bbox::BBox;
pub use chain::chain_segments;
pub use contour::{Contour, Orientation};
pub use point::Point;
pub use segment::{Circle, Segment};
pub use transform::Transform;
pub use tree::{containment_tree, ContainmentNode};

/// General floating point tolerance.
pub const EPS: f64 = 1e-9;
/// Default endpoint tolerance for chaining segments (mm).
pub const DEFAULT_CHAIN_TOL: f64 = 0.01;
