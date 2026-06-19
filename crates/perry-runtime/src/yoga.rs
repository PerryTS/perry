//! Native `yoga-layout` backend for Perry — taffy-powered flexbox so the
//! real `ink` (and any `yoga-layout` consumer) lays out natively, no WASM.
//!
//! The npm `yoga-layout` package is a WASM blob; Perry can't run WASM. ink
//! only ever talks to yoga through a fixed API (`Node.create()`, style
//! setters, `calculateLayout`, `getComputed*`, a measure callback). This
//! module reimplements that surface over `taffy` (the same Rust flexbox
//! engine `perry/tui` uses), exposed to TS as the `perry/yoga` native module.
//! A thin TS shim (shipped as the `yoga-layout` package's `index.ts`) maps
//! the public yoga API onto these primitives and keeps yoga's enum
//! constants (`YGEnums.ts`).
//!
//! Nodes are plain integer handles (an `f64` id, not a tagged pointer) into
//! a per-process registry. `calculateLayout` builds a fresh `TaffyTree` from
//! the registry, runs the solver (calling stored JS measure callbacks for
//! leaf text nodes), and writes the relative computed `Layout` back onto each
//! node for `getComputed*` to read — mirroring how `tui/layout.rs` works.

use std::collections::HashMap;
use std::sync::Mutex;

use taffy::prelude::{auto, length, percent};
use taffy::{
    AlignContent, AlignItems, AvailableSpace, Dimension, Display, FlexDirection, FlexWrap,
    JustifyContent, Layout, LengthPercentage, LengthPercentageAuto, NodeId, Overflow, Point,
    Position, Rect, Size, Style, TaffyTree,
};

use crate::value::{POINTER_MASK, POINTER_TAG, TAG_UNDEFINED};

// ---------------------------------------------------------------------------
// Property / unit / enum tags — must stay in sync with the TS shim.
// ---------------------------------------------------------------------------

// Numeric props (js_yoga_set_number)
const P_WIDTH: u32 = 0;
const P_HEIGHT: u32 = 1;
const P_MIN_WIDTH: u32 = 2;
const P_MIN_HEIGHT: u32 = 3;
const P_MAX_WIDTH: u32 = 4;
const P_MAX_HEIGHT: u32 = 5;
const P_FLEX_BASIS: u32 = 6;
const P_FLEX_GROW: u32 = 7;
const P_FLEX_SHRINK: u32 = 8;
const P_FLEX: u32 = 9;
const P_ASPECT_RATIO: u32 = 10;

// Units
const U_POINT: u32 = 0;
const U_PERCENT: u32 = 1;
const U_AUTO: u32 = 2;
const U_UNDEFINED: u32 = 3;

// Edge props (js_yoga_set_edge)
const E_MARGIN: u32 = 0;
const E_PADDING: u32 = 1;
const E_BORDER: u32 = 2;
const E_POSITION: u32 = 3;

// yoga YGEdge values
const EDGE_LEFT: u32 = 0;
const EDGE_TOP: u32 = 1;
const EDGE_RIGHT: u32 = 2;
const EDGE_BOTTOM: u32 = 3;
const EDGE_START: u32 = 4;
const EDGE_END: u32 = 5;
const EDGE_HORIZONTAL: u32 = 6;
const EDGE_VERTICAL: u32 = 7;
const EDGE_ALL: u32 = 8;

// Enum props (js_yoga_set_enum)
const EN_FLEX_DIRECTION: u32 = 0;
const EN_JUSTIFY: u32 = 1;
const EN_ALIGN_ITEMS: u32 = 2;
const EN_ALIGN_SELF: u32 = 3;
const EN_ALIGN_CONTENT: u32 = 4;
const EN_FLEX_WRAP: u32 = 5;
const EN_DISPLAY: u32 = 6;
const EN_POSITION_TYPE: u32 = 7;
const EN_OVERFLOW: u32 = 8;

// Computed-layout fields (js_yoga_get_computed)
const C_LEFT: u32 = 0;
const C_TOP: u32 = 1;
const C_WIDTH: u32 = 2;
const C_HEIGHT: u32 = 3;
const C_RIGHT: u32 = 4;
const C_BOTTOM: u32 = 5;

// ---------------------------------------------------------------------------
// Node registry
// ---------------------------------------------------------------------------

struct YogaNode {
    style: Style,
    children: Vec<u32>,
    /// NaN-boxed JS measure callback, or `TAG_UNDEFINED` for none.
    measure: f64,
    layout: Layout,
}

impl YogaNode {
    fn new() -> Self {
        YogaNode {
            style: Style {
                // yoga defaults to flex-direction: column, position: relative.
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                ..Default::default()
            },
            children: Vec::new(),
            measure: f64::from_bits(TAG_UNDEFINED),
            layout: Layout::default(),
        }
    }
}

static YOGA_NODES: Mutex<Option<HashMap<u32, YogaNode>>> = Mutex::new(None);
static YOGA_NEXT_ID: Mutex<u32> = Mutex::new(1);
static GC_SCANNER_REGISTERED: Mutex<bool> = Mutex::new(false);

fn with_nodes<R, F: FnOnce(&mut HashMap<u32, YogaNode>) -> R>(f: F) -> R {
    ensure_gc_scanner_registered();
    let mut guard = crate::gc::lock_gc_root_registry(&YOGA_NODES);
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    f(guard.as_mut().unwrap())
}

fn ensure_gc_scanner_registered() {
    let mut reg = GC_SCANNER_REGISTERED.lock().unwrap();
    if !*reg {
        crate::gc::gc_register_mutable_root_scanner_named("perry_yoga", yoga_root_scanner);
        *reg = true;
    }
}

/// Keep stored measure callbacks alive across GC between `setMeasureFunc`
/// and `calculateLayout`.
fn yoga_root_scanner(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut guard = crate::gc::lock_gc_root_registry(&YOGA_NODES);
    if let Some(map) = guard.as_mut() {
        for node in map.values_mut() {
            if (node.measure.to_bits() & !POINTER_MASK) == POINTER_TAG {
                visitor.visit_nanbox_f64_slot(&mut node.measure);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

#[inline]
fn dim(value: f32, unit: u32) -> Dimension {
    match unit {
        U_POINT => length(value),
        U_PERCENT => percent(value / 100.0),
        _ => auto(),
    }
}

#[inline]
fn lp(value: f32, unit: u32) -> LengthPercentage {
    match unit {
        U_PERCENT => percent(value / 100.0),
        _ => length(value),
    }
}

#[inline]
fn lpa(value: f32, unit: u32) -> LengthPercentageAuto {
    match unit {
        U_PERCENT => percent(value / 100.0),
        U_AUTO | U_UNDEFINED => auto(),
        _ => length(value),
    }
}

fn rect_set_edge_lp(rect: &mut Rect<LengthPercentage>, edge: u32, v: LengthPercentage) {
    match edge {
        EDGE_LEFT | EDGE_START => rect.left = v,
        EDGE_RIGHT | EDGE_END => rect.right = v,
        EDGE_TOP => rect.top = v,
        EDGE_BOTTOM => rect.bottom = v,
        EDGE_HORIZONTAL => {
            rect.left = v;
            rect.right = v;
        }
        EDGE_VERTICAL => {
            rect.top = v;
            rect.bottom = v;
        }
        EDGE_ALL => {
            rect.left = v;
            rect.right = v;
            rect.top = v;
            rect.bottom = v;
        }
        _ => {}
    }
}

fn rect_set_edge_lpa(rect: &mut Rect<LengthPercentageAuto>, edge: u32, v: LengthPercentageAuto) {
    match edge {
        EDGE_LEFT | EDGE_START => rect.left = v,
        EDGE_RIGHT | EDGE_END => rect.right = v,
        EDGE_TOP => rect.top = v,
        EDGE_BOTTOM => rect.bottom = v,
        EDGE_HORIZONTAL => {
            rect.left = v;
            rect.right = v;
        }
        EDGE_VERTICAL => {
            rect.top = v;
            rect.bottom = v;
        }
        EDGE_ALL => {
            rect.left = v;
            rect.right = v;
            rect.top = v;
            rect.bottom = v;
        }
        _ => {}
    }
}

// yoga enum value → taffy
fn map_flex_direction(v: u32) -> FlexDirection {
    match v {
        0 => FlexDirection::Column,
        1 => FlexDirection::ColumnReverse,
        2 => FlexDirection::Row,
        3 => FlexDirection::RowReverse,
        _ => FlexDirection::Column,
    }
}

fn map_justify(v: u32) -> Option<JustifyContent> {
    Some(match v {
        0 => JustifyContent::FlexStart,
        1 => JustifyContent::Center,
        2 => JustifyContent::FlexEnd,
        3 => JustifyContent::SpaceBetween,
        4 => JustifyContent::SpaceAround,
        5 => JustifyContent::SpaceEvenly,
        _ => return None,
    })
}

fn map_align(v: u32) -> Option<AlignItems> {
    Some(match v {
        1 => AlignItems::FlexStart,
        2 => AlignItems::Center,
        3 => AlignItems::FlexEnd,
        4 => AlignItems::Stretch,
        5 => AlignItems::Baseline,
        // 0 = Auto; 6/7/8 (space-*) aren't valid for align-items in taffy.
        _ => return None,
    })
}

fn map_align_content(v: u32) -> Option<AlignContent> {
    Some(match v {
        1 => AlignContent::FlexStart,
        2 => AlignContent::Center,
        3 => AlignContent::FlexEnd,
        4 => AlignContent::Stretch,
        6 => AlignContent::SpaceBetween,
        7 => AlignContent::SpaceAround,
        8 => AlignContent::SpaceEvenly,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// FFI: node lifecycle
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_yoga_node_new() -> f64 {
    let id = {
        let mut n = YOGA_NEXT_ID.lock().unwrap();
        let v = *n;
        *n += 1;
        v
    };
    with_nodes(|m| {
        m.insert(id, YogaNode::new());
    });
    id as f64
}

#[no_mangle]
pub extern "C" fn js_yoga_node_free(id: f64) -> f64 {
    let id = id as u32;
    with_nodes(|m| {
        m.remove(&id);
    });
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_yoga_insert_child(parent: f64, child: f64, index: f64) -> f64 {
    let (p, c, idx) = (parent as u32, child as u32, index as usize);
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&p) {
            let idx = idx.min(node.children.len());
            node.children.insert(idx, c);
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_yoga_remove_child(parent: f64, child: f64) -> f64 {
    let (p, c) = (parent as u32, child as u32);
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&p) {
            node.children.retain(|&x| x != c);
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_yoga_child_count(id: f64) -> f64 {
    let id = id as u32;
    with_nodes(|m| m.get(&id).map(|n| n.children.len()).unwrap_or(0) as f64)
}

#[no_mangle]
pub extern "C" fn js_yoga_set_measure_func(id: f64, cb: f64) -> f64 {
    let id = id as u32;
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&id) {
            node.measure = cb;
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_yoga_unset_measure_func(id: f64) -> f64 {
    let id = id as u32;
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&id) {
            node.measure = f64::from_bits(TAG_UNDEFINED);
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

// ---------------------------------------------------------------------------
// FFI: style setters
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_yoga_set_number(id: f64, prop: f64, value: f64, unit: f64) -> f64 {
    let (id, prop, unit, v) = (id as u32, prop as u32, unit as u32, value as f32);
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&id) {
            let s = &mut node.style;
            match prop {
                P_WIDTH => s.size.width = dim(v, unit),
                P_HEIGHT => s.size.height = dim(v, unit),
                P_MIN_WIDTH => s.min_size.width = dim(v, unit),
                P_MIN_HEIGHT => s.min_size.height = dim(v, unit),
                P_MAX_WIDTH => s.max_size.width = dim(v, unit),
                P_MAX_HEIGHT => s.max_size.height = dim(v, unit),
                P_FLEX_BASIS => s.flex_basis = dim(v, unit),
                P_FLEX_GROW => s.flex_grow = v,
                P_FLEX_SHRINK => s.flex_shrink = v,
                P_FLEX
                    // yoga setFlex(v): grow=v, shrink=v, basis=0 when v>0.
                    if v >= 0.0 => {
                        s.flex_grow = v;
                        s.flex_shrink = v;
                    }
                P_ASPECT_RATIO => s.aspect_ratio = if v.is_finite() { Some(v) } else { None },
                _ => {}
            }
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_yoga_set_edge(id: f64, prop: f64, edge: f64, value: f64, unit: f64) -> f64 {
    let (id, prop, edge, unit, v) = (
        id as u32,
        prop as u32,
        edge as u32,
        unit as u32,
        value as f32,
    );
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&id) {
            let s = &mut node.style;
            match prop {
                E_MARGIN => rect_set_edge_lpa(&mut s.margin, edge, lpa(v, unit)),
                E_POSITION => rect_set_edge_lpa(&mut s.inset, edge, lpa(v, unit)),
                E_PADDING => rect_set_edge_lp(&mut s.padding, edge, lp(v, unit)),
                E_BORDER => rect_set_edge_lp(&mut s.border, edge, lp(v, unit)),
                _ => {}
            }
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_yoga_set_gap(id: f64, gutter: f64, value: f64, unit: f64) -> f64 {
    let (id, gutter, unit, v) = (id as u32, gutter as u32, unit as u32, value as f32);
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&id) {
            let s = &mut node.style;
            let g = lp(v, unit);
            match gutter {
                0 => s.gap.width = g,  // YGGutterColumn
                1 => s.gap.height = g, // YGGutterRow
                _ => {
                    s.gap.width = g;
                    s.gap.height = g;
                } // YGGutterAll
            }
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_yoga_set_enum(id: f64, prop: f64, value: f64) -> f64 {
    let (id, prop, v) = (id as u32, prop as u32, value as u32);
    with_nodes(|m| {
        if let Some(node) = m.get_mut(&id) {
            let s = &mut node.style;
            match prop {
                EN_FLEX_DIRECTION => s.flex_direction = map_flex_direction(v),
                EN_JUSTIFY => s.justify_content = map_justify(v),
                EN_ALIGN_ITEMS => s.align_items = map_align(v),
                EN_ALIGN_SELF => s.align_self = map_align(v),
                EN_ALIGN_CONTENT => s.align_content = map_align_content(v),
                EN_FLEX_WRAP => {
                    s.flex_wrap = match v {
                        1 => FlexWrap::Wrap,
                        2 => FlexWrap::WrapReverse,
                        _ => FlexWrap::NoWrap,
                    }
                }
                EN_DISPLAY => {
                    s.display = match v {
                        1 => Display::None,
                        _ => Display::Flex,
                    }
                }
                EN_POSITION_TYPE => {
                    s.position = match v {
                        2 => Position::Absolute,
                        _ => Position::Relative,
                    }
                }
                EN_OVERFLOW => {
                    let o = match v {
                        1 => Overflow::Hidden,
                        2 => Overflow::Scroll,
                        _ => Overflow::Visible,
                    };
                    s.overflow = Point { x: o, y: o };
                }
                _ => {}
            }
        }
    });
    f64::from_bits(TAG_UNDEFINED)
}

// ---------------------------------------------------------------------------
// FFI: layout
// ---------------------------------------------------------------------------

const YG_MEASURE_UNDEFINED: f64 = 0.0;
const YG_MEASURE_EXACTLY: f64 = 1.0;
const YG_MEASURE_AT_MOST: f64 = 2.0;

#[no_mangle]
pub extern "C" fn js_yoga_calculate_layout(
    id: f64,
    width: f64,
    height: f64,
    _direction: f64,
) -> f64 {
    let root_id = id as u32;

    // Phase 1: build a fresh taffy tree from the registry, collecting the
    // (taffy NodeId -> measure callback) map for leaves. Done under the lock,
    // but we release it before computing so measure callbacks (which re-enter
    // JS and may call yoga functions) don't deadlock.
    let mut taffy: TaffyTree<u32> = TaffyTree::new();
    let mut handle_to_taffy: HashMap<u32, NodeId> = HashMap::new();
    // Keyed by yoga handle (the taffy node context), so the measure closure
    // can resolve the callback straight from the context without the map.
    let mut measure_cbs: HashMap<u32, f64> = HashMap::new();

    let built = with_nodes(|m| {
        build_taffy(
            m,
            &mut taffy,
            &mut handle_to_taffy,
            &mut measure_cbs,
            root_id,
        )
    });
    let root_taffy = match built {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };

    let avail = Size {
        width: if width.is_finite() {
            AvailableSpace::Definite(width as f32)
        } else {
            AvailableSpace::MaxContent
        },
        height: if height.is_finite() {
            AvailableSpace::Definite(height as f32)
        } else {
            AvailableSpace::MaxContent
        },
    };

    let _ = taffy.compute_layout_with_measure(
        root_taffy,
        avail,
        |known_dims, available_space, _node_id, node_context, _style| {
            let cb = node_context.and_then(|yid| measure_cbs.get(&*yid).copied());
            measure_leaf(known_dims, available_space, cb)
        },
    );

    // Phase 3: write computed layouts back onto each node.
    with_nodes(|m| {
        store_layout(m, &taffy, &handle_to_taffy, root_id);
    });

    f64::from_bits(TAG_UNDEFINED)
}

fn build_taffy(
    m: &HashMap<u32, YogaNode>,
    taffy: &mut TaffyTree<u32>,
    map: &mut HashMap<u32, NodeId>,
    measure_cbs: &mut HashMap<u32, f64>,
    handle: u32,
) -> Option<NodeId> {
    let node = m.get(&handle)?;
    let style = node.style.clone();
    let has_measure = (node.measure.to_bits() & !POINTER_MASK) == POINTER_TAG;

    if node.children.is_empty() {
        // Leaf — attach the yoga handle as context so the measure closure
        // can find this node's callback.
        let tid = taffy.new_leaf_with_context(style, handle).ok()?;
        map.insert(handle, tid);
        if has_measure {
            measure_cbs.insert(handle, node.measure);
        }
        Some(tid)
    } else {
        let child_ids: Vec<NodeId> = node
            .children
            .iter()
            .filter_map(|c| build_taffy(m, taffy, map, measure_cbs, *c))
            .collect();
        let tid = taffy.new_with_children(style, &child_ids).ok()?;
        map.insert(handle, tid);
        Some(tid)
    }
}

fn measure_leaf(
    known_dims: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    cb: Option<f64>,
) -> Size<f32> {
    let cb = match cb {
        Some(c) => c,
        None => {
            return Size {
                width: known_dims.width.unwrap_or(0.0),
                height: known_dims.height.unwrap_or(0.0),
            }
        }
    };

    // Build yoga measure args: (width, widthMode, height, heightMode).
    let (w, wmode) = measure_axis(known_dims.width, available_space.width);
    let (h, hmode) = measure_axis(known_dims.height, available_space.height);
    let args = [w, wmode, h, hmode];

    let result = unsafe { crate::closure::js_native_call_value(cb, args.as_ptr(), 4) };

    // result is a NaN-boxed object {width, height}; read both numbers.
    let bits = result.to_bits();
    if (bits & !POINTER_MASK) != POINTER_TAG {
        return Size {
            width: known_dims.width.unwrap_or(0.0),
            height: known_dims.height.unwrap_or(0.0),
        };
    }
    let obj = (bits & POINTER_MASK) as *const crate::object::ObjectHeader;
    let rw = read_obj_number(obj, b"width");
    let rh = read_obj_number(obj, b"height");
    Size {
        width: rw.unwrap_or_else(|| known_dims.width.unwrap_or(0.0)),
        height: rh.unwrap_or_else(|| known_dims.height.unwrap_or(0.0)),
    }
}

fn measure_axis(known: Option<f32>, avail: AvailableSpace) -> (f64, f64) {
    if let Some(k) = known {
        (k as f64, YG_MEASURE_EXACTLY)
    } else {
        match avail {
            AvailableSpace::Definite(v) => (v as f64, YG_MEASURE_AT_MOST),
            _ => (f64::NAN, YG_MEASURE_UNDEFINED),
        }
    }
}

fn read_obj_number(obj: *const crate::object::ObjectHeader, name: &[u8]) -> Option<f32> {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    if key.is_null() {
        return None;
    }
    let v = crate::object::js_object_get_field_by_name_f64(obj, key);
    if v.is_finite() {
        Some(v as f32)
    } else {
        None
    }
}

fn store_layout(
    m: &mut HashMap<u32, YogaNode>,
    taffy: &TaffyTree<u32>,
    map: &HashMap<u32, NodeId>,
    handle: u32,
) {
    if let Some(tid) = map.get(&handle) {
        if let Ok(l) = taffy.layout(*tid) {
            if let Some(node) = m.get_mut(&handle) {
                node.layout = *l;
            }
        }
    }
    // Recurse (children list is on the node; clone to avoid borrow clash).
    let children = m
        .get(&handle)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for c in children {
        store_layout(m, taffy, map, c);
    }
}

#[no_mangle]
pub extern "C" fn js_yoga_get_computed(id: f64, field: f64) -> f64 {
    let (id, field) = (id as u32, field as u32);
    with_nodes(|m| {
        m.get(&id)
            .map(|n| {
                let l = &n.layout;
                match field {
                    C_LEFT => l.location.x as f64,
                    C_TOP => l.location.y as f64,
                    C_WIDTH => l.size.width as f64,
                    C_HEIGHT => l.size.height as f64,
                    C_RIGHT => (l.location.x + l.size.width) as f64,
                    C_BOTTOM => (l.location.y + l.size.height) as f64,
                    _ => 0.0,
                }
            })
            .unwrap_or(0.0)
    })
}

#[no_mangle]
pub extern "C" fn js_yoga_get_computed_edge(id: f64, kind: f64, edge: f64) -> f64 {
    let (id, kind, edge) = (id as u32, kind as u32, edge as u32);
    with_nodes(|m| {
        m.get(&id)
            .map(|n| {
                let l = &n.layout;
                // taffy's Layout exposes resolved padding/border; margin is
                // collapsed into positioning, so report 0 for it (ink only
                // relies on getComputedPadding/getComputedBorder).
                let r = match kind {
                    E_PADDING => &l.padding,
                    E_BORDER => &l.border,
                    _ => return 0.0,
                };
                match edge {
                    EDGE_LEFT | EDGE_START => r.left as f64,
                    EDGE_RIGHT | EDGE_END => r.right as f64,
                    EDGE_TOP => r.top as f64,
                    EDGE_BOTTOM => r.bottom as f64,
                    _ => 0.0,
                }
            })
            .unwrap_or(0.0)
    })
}

// Keep-alive anchors so whole-program LTO doesn't strip the #[no_mangle] FFI
// symbols that are only referenced from generated `.o` (native-table dispatch).
// Typed statics (coercion, not a const ptr→int cast).
macro_rules! keep {
    ($n:ident : $t:ty = $f:ident) => {
        #[used]
        static $n: $t = $f;
    };
}
keep!(KEEP_YOGA_0: extern "C" fn() -> f64 = js_yoga_node_new);
keep!(KEEP_YOGA_1: extern "C" fn(f64) -> f64 = js_yoga_node_free);
keep!(KEEP_YOGA_2: extern "C" fn(f64, f64, f64) -> f64 = js_yoga_insert_child);
keep!(KEEP_YOGA_3: extern "C" fn(f64, f64) -> f64 = js_yoga_remove_child);
keep!(KEEP_YOGA_4: extern "C" fn(f64) -> f64 = js_yoga_child_count);
keep!(KEEP_YOGA_5: extern "C" fn(f64, f64) -> f64 = js_yoga_set_measure_func);
keep!(KEEP_YOGA_6: extern "C" fn(f64) -> f64 = js_yoga_unset_measure_func);
keep!(KEEP_YOGA_7: extern "C" fn(f64, f64, f64, f64) -> f64 = js_yoga_set_number);
keep!(KEEP_YOGA_8: extern "C" fn(f64, f64, f64, f64, f64) -> f64 = js_yoga_set_edge);
keep!(KEEP_YOGA_9: extern "C" fn(f64, f64, f64, f64) -> f64 = js_yoga_set_gap);
keep!(KEEP_YOGA_10: extern "C" fn(f64, f64, f64) -> f64 = js_yoga_set_enum);
keep!(KEEP_YOGA_11: extern "C" fn(f64, f64, f64, f64) -> f64 = js_yoga_calculate_layout);
keep!(KEEP_YOGA_12: extern "C" fn(f64, f64) -> f64 = js_yoga_get_computed);
keep!(KEEP_YOGA_13: extern "C" fn(f64, f64, f64) -> f64 = js_yoga_get_computed_edge);
