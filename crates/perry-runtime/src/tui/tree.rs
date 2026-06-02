//! Widget tree. v0.1 keeps the model deliberately small: every widget
//! is one of a handful of variants, registered in a global handle
//! table so the FFI can refer to them by integer id (matching how
//! perry/ui's widget handles work).
//!
//! Phase 3 (Taffy integration) extends this with layout props on Box
//! (flexDirection, justifyContent, alignItems, padding, …); for now
//! Box is a vertical stack of its children.

use super::cell::{Grid, Style};
use super::color::Color;
use super::style::BoxStyle;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

/// Widget node. `Box` carries an ordered Vec of child handles so the
/// parent → child relationship is explicit (children are paint roots
/// in their own right; we don't store a back-ref).
///
/// Phase 3 (#358) attached a `BoxStyle` to every Box for the Taffy
/// flexbox solver — `flexDirection`, `justifyContent`, `alignItems`,
/// `gap`, `padding`, explicit `width`/`height`. Defaults match the
/// v0.1 vertical-stack behavior so existing code keeps working
/// without supplying a style.
#[derive(Clone, Debug)]
pub enum Node {
    Box {
        children: Vec<i64>,
        fg: Color,
        bg: Color,
        style: BoxStyle,
    },
    Text {
        content: String,
        fg: Color,
        bg: Color,
        style: Style,
    },
}

/// Global handle table. Allocations come from `NEXT_HANDLE`; lookups
/// through the table take the lock for the duration of a get/set. The
/// lock isn't on the hot path (FFI calls fire on the main thread once
/// per render) so a plain Mutex is fine.
static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static REGISTRY: Mutex<Vec<(i64, Node)>> = Mutex::new(Vec::new());

/// Register a freshly-built node and return its handle.
pub fn register(node: Node) -> i64 {
    let h = NEXT_HANDLE.fetch_add(1, Ordering::AcqRel);
    REGISTRY.lock().unwrap().push((h, node));
    h
}

/// Look up a node by handle. Returns `None` if the handle is unknown
/// (e.g. it was 0, or the handle was dropped).
pub fn lookup(handle: i64) -> Option<Node> {
    let reg = REGISTRY.lock().unwrap();
    reg.iter()
        .find_map(|(h, n)| if *h == handle { Some(n.clone()) } else { None })
}

/// Append a child handle to a Box node. No-op if the handle isn't a
/// Box (silently ignored — matches the "we accept anything, you check
/// at the call site" convention from the rest of Perry's FFI).
pub fn box_add_child(parent: i64, child: i64) {
    let mut reg = REGISTRY.lock().unwrap();
    for (h, n) in reg.iter_mut() {
        if *h == parent {
            if let Node::Box { children, .. } = n {
                children.push(child);
            }
            return;
        }
    }
}

/// Run a closure with mutable access to a node by handle. Returns
/// `true` if the handle was found, `false` otherwise. Used by the FFI
/// box-style setters in `ffi.rs` so the lock is acquired once per
/// mutation.
pub fn with_node_mut(handle: i64, f: impl FnOnce(&mut Node)) -> bool {
    let mut reg = REGISTRY.lock().unwrap();
    for (h, n) in reg.iter_mut() {
        if *h == handle {
            f(n);
            return true;
        }
    }
    false
}

/// Total number of nodes currently registered. Test-only.
#[doc(hidden)]
#[cfg(test)]
pub fn registry_len() -> usize {
    REGISTRY.lock().unwrap().len()
}

/// Paint a node tree into the given grid using the v0.1 vertical-
/// stack semantics (each Box child advances row by 1). Used as a
/// fallback when no layout pass has been run, e.g. unit tests that
/// pre-date Phase 3's Taffy integration. Returns the next available
/// row.
pub fn paint(grid: &mut Grid, root: i64, row: u16, col: u16) -> u16 {
    let node = match lookup(root) {
        Some(n) => n,
        None => return row,
    };
    match node {
        Node::Text {
            content,
            fg,
            bg,
            style,
        } => {
            grid.paint_text(row, col, &content, fg, bg, style);
            row + 1
        }
        Node::Box { children, .. } => {
            let mut r = row;
            for child in children {
                r = paint(grid, child, r, col);
            }
            r
        }
    }
}

/// Paint using a precomputed Taffy layout map. Each node's rect is
/// looked up by its handle; Text nodes paint at the stored (row, col)
/// position, Box nodes don't paint themselves (they're invisible
/// containers in v0.3 — borders / backgrounds are Phase 3.5). Out-
/// of-tree handles or missing rects are silently dropped.
pub fn paint_with_layout(
    grid: &mut Grid,
    root: i64,
    rects: &std::collections::HashMap<i64, super::layout::Rect>,
) {
    let node = match lookup(root) {
        Some(n) => n,
        None => return,
    };
    let rect = match rects.get(&root) {
        Some(r) => *r,
        None => return,
    };
    match node {
        Node::Text {
            content,
            fg,
            bg,
            style,
        } => {
            grid.paint_text(rect.row, rect.col, &content, fg, bg, style);
        }
        Node::Box { children, .. } => {
            for child in children {
                paint_with_layout(grid, child, rects);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_single_text() {
        let h = register(Node::Text {
            content: "hello".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let mut g = Grid::new(10, 3);
        let after = paint(&mut g, h, 0, 0);
        assert_eq!(after, 1);
        assert_eq!(g.back[0].ch, 'h');
        assert_eq!(g.back[4].ch, 'o');
    }

    #[test]
    fn paint_box_stacks_children_vertically() {
        let t1 = register(Node::Text {
            content: "abc".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let t2 = register(Node::Text {
            content: "def".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let b = register(Node::Box {
            children: vec![],
            fg: Color::Default,
            bg: Color::Default,
            style: BoxStyle::default(),
        });
        box_add_child(b, t1);
        box_add_child(b, t2);
        let mut g = Grid::new(5, 3);
        let after = paint(&mut g, b, 0, 0);
        assert_eq!(after, 2);
        // Row 0 has "abc"
        assert_eq!(g.back[0].ch, 'a');
        assert_eq!(g.back[1].ch, 'b');
        assert_eq!(g.back[2].ch, 'c');
        // Row 1 has "def"
        let row1 = 5;
        assert_eq!(g.back[row1].ch, 'd');
        assert_eq!(g.back[row1 + 1].ch, 'e');
        assert_eq!(g.back[row1 + 2].ch, 'f');
    }

    #[test]
    fn lookup_returns_none_for_invalid_handle() {
        assert!(lookup(99_999).is_none());
        assert!(lookup(0).is_none());
    }

    #[test]
    fn register_increments_handle() {
        let n_before = registry_len();
        let h1 = register(Node::Text {
            content: "x".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let h2 = register(Node::Text {
            content: "y".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        assert_eq!(h2, h1 + 1);
        assert_eq!(registry_len(), n_before + 2);
    }
}
