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

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

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

per_test_global! {
    static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
    static REGISTRY: LazyLock<Mutex<HashMap<i64, Node>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
}

/// Register a freshly-built node and return its handle.
pub fn register(node: Node) -> i64 {
    let h = NEXT_HANDLE.fetch_add(1, Ordering::AcqRel);
    REGISTRY.lock().unwrap().insert(h, node);
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(crate::hot_diag::ReceiverReprFamily::Tui);
    }
    h
}

/// Look up a node by handle. Returns `None` if the handle is unknown
/// (e.g. it was 0, or the handle was dropped).
pub fn lookup(handle: i64) -> Option<Node> {
    REGISTRY.lock().unwrap().get(&handle).cloned()
}

/// Append a child handle to a Box node. No-op if the handle isn't a
/// Box (silently ignored — matches the "we accept anything, you check
/// at the call site" convention from the rest of Perry's FFI).
pub fn box_add_child(parent: i64, child: i64) {
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(Node::Box { children, .. }) = reg.get_mut(&parent) {
        children.push(child);
    }
}

/// Run a closure with mutable access to a node by handle. Returns
/// `true` if the handle was found, `false` otherwise. Used by the FFI
/// box-style setters in `ffi.rs` so the lock is acquired once per
/// mutation.
pub fn with_node_mut(handle: i64, f: impl FnOnce(&mut Node)) -> bool {
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(node) = reg.get_mut(&handle) {
        f(node);
        return true;
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

    fn text(content: &str) -> i64 {
        register(Node::Text {
            content: content.to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        })
    }

    #[test]
    fn handles_and_mutations_survive_registry_growth() {
        let first = text("first");
        let parent = register(Node::Box {
            children: vec![first],
            fg: Color::Default,
            bg: Color::Default,
            style: BoxStyle::default(),
        });
        for _ in 0..10_000 {
            text("unrelated");
        }
        let last = text("last");
        box_add_child(parent, last);
        assert!(with_node_mut(first, |node| {
            if let Node::Text { content, .. } = node {
                *content = "changed".to_string();
            }
        }));
        let Some(Node::Box { children, .. }) = lookup(parent) else {
            panic!("parent must retain its identity");
        };
        assert_eq!(children, vec![first, last]);
        let mut grid = Grid::new(10, 2);
        assert_eq!(paint(&mut grid, parent, 0, 0), 2);
        assert_eq!(grid.back[0].ch, 'c');
        assert_eq!(grid.back[10].ch, 'l');
    }

    #[test]
    fn lookup_returns_an_independent_snapshot() {
        let handle = text("original");
        let Some(Node::Text { mut content, .. }) = lookup(handle) else {
            panic!("text must be registered");
        };
        content.clear();
        let Some(Node::Text { content, .. }) = lookup(handle) else {
            panic!("text must remain registered");
        };
        assert_eq!(content, "original");
    }

    #[test]
    fn invalid_mutations_and_non_box_children_are_noops() {
        let handle = text("unchanged");
        box_add_child(handle, handle);
        box_add_child(0, handle);
        assert!(!with_node_mut(0, |_| panic!("unknown handle was mutated")));
        let Some(Node::Text { content, .. }) = lookup(handle) else {
            panic!("adding a child must not change a text node's kind");
        };
        assert_eq!(content, "unchanged");
    }

    #[test]
    #[ignore = "manual registry scaling benchmark"]
    fn registry_scaling_benchmark() {
        use std::hint::black_box;
        use std::time::Instant;

        for size in [100, 10_000, 100_000] {
            while registry_len() < size {
                text("unrelated");
            }
            let start = Instant::now();
            let children = (0..32).map(|_| text("active")).collect();
            let root = register(Node::Box {
                children,
                fg: Color::Default,
                bg: Color::Default,
                style: BoxStyle::default(),
            });
            let build = start.elapsed();
            let start = Instant::now();
            for _ in 0..10_000 {
                black_box(lookup(root));
            }
            let lookup_time = start.elapsed();
            let start = Instant::now();
            let rects = super::super::layout::compute_layout(root, 80, 40);
            let layout_time = start.elapsed();
            let mut grid = Grid::new(80, 40);
            let start = Instant::now();
            for _ in 0..100 {
                paint_with_layout(&mut grid, root, &rects);
            }
            eprintln!(
                "registry={} build={build:?} lookup={lookup_time:?} layout={layout_time:?} paint={:?}",
                registry_len(),
                start.elapsed()
            );
            assert_eq!(grid.back[0].ch, 'a');
        }
    }

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
