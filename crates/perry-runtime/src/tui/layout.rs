//! Taffy-driven layout pass. Walks the perry/tui Node tree, mirrors
//! it into a TaffyTree<()> with translated styles, runs the flexbox
//! solver, and returns a `HashMap<i64, Rect>` from widget handle to
//! computed cell rect that the painter consumes.
//!
//! Cells are integer units; Taffy uses float points internally. We
//! seed with cell counts as float dimensions and round the output
//! back to integers — for terminal-cell layouts the rounding error
//! is bounded since gaps and paddings are user-supplied integers.
//!
//! v0.3 (Phase 3 of #358) supports: `display: flex`, `flex-direction`,
//! `justify-content`, `align-items`, `gap`, `padding`, explicit
//! `width`/`height`. `flex-grow`/`flex-shrink`/`flex-basis` and
//! percentage units are deferred to Phase 3.5.

use std::collections::HashMap;

use taffy::prelude::{auto, length, percent, AvailableSpace, NodeId, Size, Style};
use taffy::{Dimension, Display, LengthPercentage, Rect as TaffyRect, TaffyTree};

use super::style::{AlignItems, BoxStyle, Edges, FlexDirection, JustifyContent, Length as TuiLen};
use super::tree::{lookup, Node};

/// Computed pixel-rect for one widget. Cells, not points; integer
/// origin + size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub row: u16,
    pub col: u16,
    pub width: u16,
    pub height: u16,
}

/// Compute Taffy layout for the tree rooted at `root_handle`, given
/// the available terminal size in cells. Returns a map from widget
/// handle to its computed cell rect. Handles outside the visible
/// area still receive a rect — the painter clips at draw time.
pub fn compute_layout(root_handle: i64, term_w: u16, term_h: u16) -> HashMap<i64, Rect> {
    let mut taffy: TaffyTree<()> = TaffyTree::new();
    let mut handle_to_taffy: HashMap<i64, NodeId> = HashMap::new();
    let root_node_id = match build_taffy_tree(&mut taffy, &mut handle_to_taffy, root_handle) {
        Some(id) => id,
        None => return HashMap::new(),
    };

    let avail = Size {
        width: AvailableSpace::Definite(term_w as f32),
        height: AvailableSpace::Definite(term_h as f32),
    };
    if taffy.compute_layout(root_node_id, avail).is_err() {
        return HashMap::new();
    }

    let mut rects: HashMap<i64, Rect> = HashMap::new();
    walk_layout(&taffy, &handle_to_taffy, root_handle, 0, 0, &mut rects);
    rects
}

/// Recursively mirror the perry/tui tree into Taffy. Returns the
/// root TaffyNodeId, or None if the handle isn't found. Each handle
/// is added to `handle_to_taffy` for later lookup.
fn build_taffy_tree(
    taffy: &mut TaffyTree<()>,
    map: &mut HashMap<i64, NodeId>,
    handle: i64,
) -> Option<NodeId> {
    let node = lookup(handle)?;
    match node {
        Node::Text { content, .. } => {
            // Text leaf — size = 1 row × content length cols. Use
            // char count, not byte length, so multi-byte UTF-8
            // characters don't over-size the cell rect.
            let cols = content.chars().count() as f32;
            let style = Style {
                size: Size {
                    width: length(cols),
                    height: length(1.0),
                },
                ..Default::default()
            };
            let id = taffy.new_leaf(style).ok()?;
            map.insert(handle, id);
            Some(id)
        }
        Node::Box {
            children, style, ..
        } => {
            let mut child_ids: Vec<NodeId> = Vec::new();
            for c in &children {
                if let Some(id) = build_taffy_tree(taffy, map, *c) {
                    child_ids.push(id);
                }
            }
            let taffy_style = box_style_to_taffy(&style);
            let id = taffy.new_with_children(taffy_style, &child_ids).ok()?;
            map.insert(handle, id);
            Some(id)
        }
    }
}

/// Convert a `Length` to Taffy's `Dimension`. Used for size /
/// flex-basis where the value can be auto / cells / percent.
fn dim_from_length(l: Option<TuiLen>) -> Dimension {
    match l {
        None => auto(),
        Some(TuiLen::Cells(n)) => length(n as f32),
        Some(TuiLen::PercentBp(bp)) => percent(bp as f32 / 10000.0),
    }
}

/// Convert a `Length` to Taffy's `LengthPercentage`. Used for
/// padding / gap where `auto` isn't a valid value.
fn lp_from_length(l: TuiLen) -> LengthPercentage {
    match l {
        TuiLen::Cells(n) => length(n as f32),
        TuiLen::PercentBp(bp) => percent(bp as f32 / 10000.0),
    }
}

/// Convert per-side cell counts to a Taffy padding rect.
fn padding_from_edges(e: Edges) -> TaffyRect<LengthPercentage> {
    TaffyRect {
        top: length(e.top as f32),
        right: length(e.right as f32),
        bottom: length(e.bottom as f32),
        left: length(e.left as f32),
    }
}

/// Translate our compact BoxStyle into Taffy's flexbox Style.
fn box_style_to_taffy(s: &BoxStyle) -> Style {
    let dir = match s.flex_direction {
        FlexDirection::Row => taffy::FlexDirection::Row,
        FlexDirection::Column => taffy::FlexDirection::Column,
    };
    let justify = match s.justify_content {
        JustifyContent::Start => Some(taffy::JustifyContent::Start),
        JustifyContent::Center => Some(taffy::JustifyContent::Center),
        JustifyContent::End => Some(taffy::JustifyContent::End),
        JustifyContent::SpaceBetween => Some(taffy::JustifyContent::SpaceBetween),
        JustifyContent::SpaceAround => Some(taffy::JustifyContent::SpaceAround),
    };
    let align = match s.align_items {
        AlignItems::Start => Some(taffy::AlignItems::Start),
        AlignItems::Center => Some(taffy::AlignItems::Center),
        AlignItems::End => Some(taffy::AlignItems::End),
        AlignItems::Stretch => Some(taffy::AlignItems::Stretch),
    };
    Style {
        display: Display::Flex,
        flex_direction: dir,
        justify_content: justify,
        align_items: align,
        flex_grow: s.flex_grow as f32,
        flex_shrink: s.flex_shrink as f32,
        flex_basis: dim_from_length(s.flex_basis),
        gap: Size {
            width: lp_from_length(TuiLen::Cells(s.gap)),
            height: lp_from_length(TuiLen::Cells(s.gap)),
        },
        padding: padding_from_edges(s.padding),
        size: Size {
            width: dim_from_length(s.width),
            height: dim_from_length(s.height),
        },
        ..Default::default()
    }
}

/// Recursively read computed layout from Taffy and accumulate into the
/// rects map. `parent_row` / `parent_col` track the absolute origin —
/// Taffy gives relative positions, we sum into absolute terminal
/// coordinates.
fn walk_layout(
    taffy: &TaffyTree<()>,
    map: &HashMap<i64, NodeId>,
    handle: i64,
    parent_row: u16,
    parent_col: u16,
    out: &mut HashMap<i64, Rect>,
) {
    let taffy_id = match map.get(&handle) {
        Some(id) => *id,
        None => return,
    };
    let layout = match taffy.layout(taffy_id) {
        Ok(l) => l,
        Err(_) => return,
    };
    let row = parent_row.saturating_add(layout.location.y as u16);
    let col = parent_col.saturating_add(layout.location.x as u16);
    let width = layout.size.width as u16;
    let height = layout.size.height as u16;
    out.insert(
        handle,
        Rect {
            row,
            col,
            width,
            height,
        },
    );
    if let Some(node) = lookup(handle) {
        if let Node::Box { children, .. } = node {
            for c in &children {
                walk_layout(taffy, map, *c, row, col, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::cell::Style;
    use super::super::color::Color;
    use super::super::tree::{box_add_child, register, Node};
    use super::*;

    #[test]
    fn vertical_stack_default_lays_out_one_per_row() {
        let t1 = register(Node::Text {
            content: "abc".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let t2 = register(Node::Text {
            content: "de".to_string(),
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

        let rects = compute_layout(b, 80, 24);
        let r1 = rects[&t1];
        let r2 = rects[&t2];
        // Default flexDirection: column → t1 at row 0, t2 at row 1.
        assert_eq!(r1.row, 0);
        assert_eq!(r2.row, 1);
        // Same column origin (no horizontal layout).
        assert_eq!(r1.col, 0);
        assert_eq!(r2.col, 0);
    }

    #[test]
    fn horizontal_stack_lays_out_side_by_side() {
        let t1 = register(Node::Text {
            content: "abc".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let t2 = register(Node::Text {
            content: "de".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let mut row_style = BoxStyle::default();
        row_style.flex_direction = FlexDirection::Row;
        let b = register(Node::Box {
            children: vec![],
            fg: Color::Default,
            bg: Color::Default,
            style: row_style,
        });
        box_add_child(b, t1);
        box_add_child(b, t2);

        let rects = compute_layout(b, 80, 24);
        let r1 = rects[&t1];
        let r2 = rects[&t2];
        // Row direction → both on row 0, t2 to the right of t1.
        assert_eq!(r1.row, 0);
        assert_eq!(r2.row, 0);
        // t2.col = t1.col + t1.width (3 chars in "abc").
        assert_eq!(r1.col, 0);
        assert_eq!(r2.col, 3);
    }

    #[test]
    fn gap_inserts_blank_cells_between_children() {
        let t1 = register(Node::Text {
            content: "x".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let t2 = register(Node::Text {
            content: "y".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let mut s = BoxStyle::default();
        s.flex_direction = FlexDirection::Row;
        s.gap = 2;
        let b = register(Node::Box {
            children: vec![],
            fg: Color::Default,
            bg: Color::Default,
            style: s,
        });
        box_add_child(b, t1);
        box_add_child(b, t2);

        let rects = compute_layout(b, 80, 24);
        let r1 = rects[&t1];
        let r2 = rects[&t2];
        // t1 at col 0 width 1 → t2 at col 0+1+gap(2) = 3.
        assert_eq!(r1.col, 0);
        assert_eq!(r2.col, 3);
    }

    #[test]
    fn padding_offsets_first_child() {
        let t = register(Node::Text {
            content: "x".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let mut s = BoxStyle::default();
        s.padding = Edges::all(2);
        let b = register(Node::Box {
            children: vec![],
            fg: Color::Default,
            bg: Color::Default,
            style: s,
        });
        box_add_child(b, t);

        let rects = compute_layout(b, 80, 24);
        let r = rects[&t];
        // Padding 2 on every side → child starts at (2, 2).
        assert_eq!(r.row, 2);
        assert_eq!(r.col, 2);
    }

    #[test]
    fn per_side_padding_only_offsets_specified_sides() {
        let t = register(Node::Text {
            content: "x".to_string(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        });
        let mut s = BoxStyle::default();
        s.padding = Edges {
            top: 1,
            right: 0,
            bottom: 0,
            left: 4,
        };
        let b = register(Node::Box {
            children: vec![],
            fg: Color::Default,
            bg: Color::Default,
            style: s,
        });
        box_add_child(b, t);

        let rects = compute_layout(b, 80, 24);
        let r = rects[&t];
        assert_eq!(r.row, 1);
        assert_eq!(r.col, 4);
    }

    #[test]
    fn percent_width_resolves_against_parent() {
        let inner = register(Node::Box {
            children: vec![],
            fg: Color::Default,
            bg: Color::Default,
            style: BoxStyle {
                width: Some(TuiLen::PercentBp(5000)),
                height: Some(TuiLen::Cells(2)),
                ..Default::default()
            },
        });
        let outer = register(Node::Box {
            children: vec![],
            fg: Color::Default,
            bg: Color::Default,
            style: BoxStyle {
                width: Some(TuiLen::Cells(40)),
                height: Some(TuiLen::Cells(10)),
                ..Default::default()
            },
        });
        box_add_child(outer, inner);

        let rects = compute_layout(outer, 80, 24);
        let r = rects[&inner];
        // 50% of 40 = 20.
        assert_eq!(r.width, 20);
        assert_eq!(r.height, 2);
    }
}
