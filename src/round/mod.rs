//! Pixel snapping as a separate pass over cumulative unrounded offsets.
//!
//! Extents are differences of rounded absolute edges so adjacent boxes never gap or overlap.

use crate::geometry::{Point, Rect, Size};
use crate::tree::{Layout, NodeId};

pub trait RoundTree {
    fn child_ids(&self, node: NodeId, out: &mut Vec<NodeId>);
    fn unrounded(&self, node: NodeId) -> Layout;
    fn set_rounded(&mut self, node: NodeId, layout: Layout);
}

/// `floor(v + 0.5)` without a libm call or saturating cast; exact for |v| < 2^22.
#[inline]
pub fn round(v: f32) -> f32 {
    let y = v + 0.5;
    let t = (y + MAGIC) - MAGIC;
    if t > y { t - 1.0 } else { t }
}

/// Adding and removing 1.5 * 2^23 leaves the nearest integer (ties to even) for |y| < 2^22:
/// the sum lands where consecutive floats are exactly one apart.
const MAGIC: f32 = 12_582_912.0;

/// [`round`] on four values at once; vectorises to a handful of SSE2 instructions.
#[inline]
fn round4(v: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0; 4];
    for k in 0..4 {
        let y = v[k] + 0.5;
        let t = (y + MAGIC) - MAGIC;
        out[k] = if t > y { t - 1.0 } else { t };
    }
    out
}

/// Rounds the whole subtree under `root`.
pub fn round_layout<T: RoundTree + ?Sized>(tree: &mut T, root: NodeId) {
    let mut stack: Vec<(NodeId, f32, f32)> = vec![(root, 0.0, 0.0)];
    let mut children = Vec::new();
    while let Some((node, cumulative_x, cumulative_y)) = stack.pop() {
        let unrounded = tree.unrounded(node);
        let cumulative_x = cumulative_x + unrounded.location.x;
        let cumulative_y = cumulative_y + unrounded.location.y;
        tree.set_rounded(node, round_one(&unrounded, cumulative_x, cumulative_y));
        children.clear();
        tree.child_ids(node, &mut children);
        for &child in children.iter().rev() {
            stack.push((child, cumulative_x, cumulative_y));
        }
    }
}

/// Rounds one layout given the absolute unrounded position of its border box.
pub fn round_one(u: &Layout, cx: f32, cy: f32) -> Layout {
    let right = cx + u.size.width;
    let bottom = cy + u.size.height;
    let [rcx, rright, rcy, rbottom] = round4([cx, right, cy, bottom]);
    let [lx, ly, sw, sh] = round4([u.location.x, u.location.y, u.scrollbar_size.width, u.scrollbar_size.height]);
    let b = round4([cx + u.border.left, right - u.border.right, cy + u.border.top, bottom - u.border.bottom]);
    let p = round4([cx + u.padding.left, right - u.padding.right, cy + u.padding.top, bottom - u.padding.bottom]);
    let m = round4([cx - u.margin.left, right + u.margin.right, cy - u.margin.top, bottom + u.margin.bottom]);
    let o = u.scrollable_overflow_rect;
    let o = round4([cx + o.left, cx + o.right, cy + o.top, cy + o.bottom]);
    Layout {
        order: u.order,
        location: Point { x: lx, y: ly },
        size: Size { width: rright - rcx, height: rbottom - rcy },
        scrollbar_size: Size { width: sw, height: sh },
        border: Rect { left: b[0] - rcx, right: rright - b[1], top: b[2] - rcy, bottom: rbottom - b[3] },
        padding: Rect { left: p[0] - rcx, right: rright - p[1], top: p[2] - rcy, bottom: rbottom - p[3] },
        margin: Rect { left: rcx - m[0], right: m[1] - rright, top: rcy - m[2], bottom: m[3] - rbottom },
        scrollable_overflow_rect: Rect { left: o[0] - rcx, right: o[1] - rcx, top: o[2] - rcy, bottom: o[3] - rcy },
    }
}

#[cfg(test)]
mod tests {
    use super::round;

    #[test]
    fn rounds_half_up_including_negatives() {
        for (v, expect) in [(2.5, 3.0), (-2.5, -2.0), (-20.0, -20.0), (-19.5, -19.0), (0.4, 0.0), (1e6 + 0.5, 1e6 + 1.0)] {
            assert_eq!(round(v), expect, "round({v})");
        }
    }
}
