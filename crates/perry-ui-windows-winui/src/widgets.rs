//! Perry widget-handle model to Windows Reactor element adapter.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::HWND;
use windows_reactor::{
    border, button as fluent_button, grid, hstack as fluent_hstack, text_block,
    vstack as fluent_vstack, Border, Brush, ButtonStyle, Callback, Color, Element, ElementExt,
    HorizontalAlignment, Modifiers, PasswordBox, ProgressBar, RenderCx, ScrollViewer, SetState,
    Slider, TextBlock, TextBox, Thickness, ToggleSwitch, VerticalAlignment,
};

use crate::winui::backend::{self, RenderBackend};

pub use perry_ui_windows::widgets::{
    ad_banner, attributed_text, bloomview, bottom_nav, calendar, canvas, chart, combobox,
    command_palette, date_picker, image, image_gallery, map_view, navstack, pdf_view, picker,
    qrcode, rich_text, rich_tooltip, table, textarea, toast, tree_view, webview, WidgetKind,
};
#[path = "../../perry-ui-windows/src/widgets/text_registry.rs"]
pub mod text_registry;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut u8;
}

#[derive(Clone, Default)]
struct Common {
    children: Vec<i64>,
    hidden: bool,
    enabled: bool,
    width: Option<f64>,
    height: Option<f64>,
    match_parent_width: bool,
    match_parent_height: bool,
    fills_remaining: bool,
    insets: (f64, f64, f64, f64),
    opacity: Option<f64>,
    background: Option<Color>,
    foreground: Option<Color>,
    border_color: Option<Color>,
    border_width: Option<f64>,
    corner_radius: Option<f64>,
    tooltip: Option<String>,
    on_click: usize,
    distribution: i64,
    alignment: i64,
    detaches_hidden: bool,
}

#[derive(Clone)]
enum NodeKind {
    Text {
        value: String,
        font_size: Option<f64>,
        font_weight: Option<u16>,
        font_family: Option<String>,
        selectable: bool,
        max_lines: Option<i32>,
        truncation: bool,
    },
    Button {
        label: String,
        callback: usize,
        bordered: bool,
    },
    VStack {
        spacing: f64,
    },
    HStack {
        spacing: f64,
    },
    ZStack,
    Spacer,
    Divider,
    TextField {
        value: String,
        placeholder: String,
        callback: usize,
        borderless: bool,
        font_size: Option<f64>,
    },
    SecureField {
        value: String,
        placeholder: String,
        callback: usize,
    },
    Toggle {
        label: String,
        on: bool,
        callback: usize,
    },
    Slider {
        min: f64,
        max: f64,
        value: f64,
        callback: usize,
    },
    ScrollView {
        offset: f64,
    },
    Form,
    Section {
        title: String,
    },
    LazyVStack {
        spacing: f64,
    },
    Progress {
        value: f64,
    },
}

#[derive(Clone)]
struct Node {
    kind: NodeKind,
    common: Common,
}

impl Node {
    fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            common: Common {
                enabled: true,
                ..Common::default()
            },
        }
    }
}

thread_local! {
    static NODES: RefCell<Vec<Node>> = const { RefCell::new(Vec::new()) };
    static ROOT: Cell<i64> = const { Cell::new(0) };
    static RENDER_EPOCH: Cell<u64> = const { Cell::new(0) };
    static RENDER_SETTER: RefCell<Option<SetState<u64>>> = const { RefCell::new(None) };
}

/// Visit the JavaScript closures the widget tree keeps alive across
/// collections.
///
/// Every callback here is a RAW CLOSURE POINTER (`callback_ptr` unboxes it via
/// `js_nanbox_get_pointer`), so each stored slot is a GC root that an
/// evacuating collection must rewrite.
///
/// KNOWN RESIDUAL (not fixable by scanning): `render_handle` works on a CLONE
/// of the node and captures the unboxed pointer by value into the `move`
/// closures it hands to Windows Reactor (`fluent_button(..).on_click(move ||
/// invoke0(selected))`, `apply_common`'s `on_tapped`, and the per-widget
/// handlers). Those captured copies live inside boxed Rust closures owned by
/// the element tree, which no scanner can reach or rewrite. Re-reading the
/// scanned `NODES` slot at invoke time — the indirection `perry-ui-macos` gets
/// from its handle-keyed callback maps — is the real fix and is a follow-up.
pub(crate) fn scan_winui_widgets_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    NODES.with(|nodes| {
        for node in nodes.borrow_mut().iter_mut() {
            // The generic `.onClick` handler every node kind can carry.
            if node.common.on_click != 0 {
                visitor.visit_usize_slot(&mut node.common.on_click);
            }
            // ...plus the per-kind handler. This match is deliberately
            // exhaustive with no `_` arm: a new `NodeKind` that stores a
            // closure has to classify itself here or fail to compile. A
            // catch-all would silently drop the new root (CLAUDE.md,
            // "Closure Captures").
            let callback = match &mut node.kind {
                NodeKind::Button { callback, .. }
                | NodeKind::TextField { callback, .. }
                | NodeKind::SecureField { callback, .. }
                | NodeKind::Toggle { callback, .. }
                | NodeKind::Slider { callback, .. } => callback,
                NodeKind::Text { .. }
                | NodeKind::VStack { .. }
                | NodeKind::HStack { .. }
                | NodeKind::ZStack
                | NodeKind::Spacer
                | NodeKind::Divider
                | NodeKind::ScrollView { .. }
                | NodeKind::Form
                | NodeKind::Section { .. }
                | NodeKind::LazyVStack { .. }
                | NodeKind::Progress { .. } => continue,
            };
            if *callback != 0 {
                visitor.visit_usize_slot(callback);
            }
        }
    });
}

fn is_fluent() -> bool {
    backend::active() == RenderBackend::Fluent
}

fn callback_ptr(value: f64) -> usize {
    unsafe { js_nanbox_get_pointer(value) as usize }
}

fn read_string(ptr: *const u8) -> String {
    unsafe { perry_ffi::copy_string_from_raw(ptr) }.to_owned()
}

fn invoke0(callback: usize) {
    if callback != 0 {
        unsafe {
            js_closure_call0(callback as *const u8);
        }
    }
}

fn invoke1(callback: usize, value: f64) {
    if callback != 0 {
        unsafe {
            js_closure_call1(callback as *const u8, value);
        }
    }
}

fn invoke_string(callback: usize, value: &str) {
    if callback == 0 {
        return;
    }
    unsafe {
        let string = js_string_from_bytes(value.as_ptr(), value.len() as u32);
        let boxed = js_nanbox_string(string as i64);
        js_closure_call1(callback as *const u8, boxed);
    }
}

fn color(r: f64, g: f64, b: f64, a: f64) -> Color {
    let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color {
        a: channel(a),
        r: channel(r),
        g: channel(g),
        b: channel(b),
    }
}

fn register(kind: NodeKind) -> i64 {
    crate::gc::ensure_registered();
    NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        nodes.push(Node::new(kind));
        nodes.len() as i64
    })
}

fn with_node_mut(handle: i64, update: impl FnOnce(&mut Node)) {
    let changed = NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        let Some(node) = nodes.get_mut(handle.saturating_sub(1) as usize) else {
            return false;
        };
        update(node);
        true
    });
    if changed {
        request_render();
    }
}

fn node(handle: i64) -> Option<Node> {
    NODES.with(|nodes| {
        nodes
            .borrow()
            .get(handle.saturating_sub(1) as usize)
            .cloned()
    })
}

pub(crate) fn set_root(handle: i64) {
    ROOT.with(|root| root.set(handle));
}

pub fn request_render() {
    if !is_fluent() {
        return;
    }
    let epoch = RENDER_EPOCH.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    RENDER_SETTER.with(|slot| {
        if let Some(setter) = slot.borrow().as_ref() {
            setter.call(epoch);
        }
    });
}

pub(crate) fn render_root(cx: &mut RenderCx) -> Element {
    let (epoch, setter) = cx.use_state(RENDER_EPOCH.with(Cell::get));
    let _ = epoch;
    RENDER_SETTER.with(|slot| *slot.borrow_mut() = Some(setter));
    crate::app::start_runtime_pump();
    let root = ROOT.with(Cell::get);
    let content = render_handle(root);
    border(content)
        .padding(Thickness::uniform(20.0))
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Stretch)
        .into()
}

fn apply_common(modifiers: &mut Modifiers, common: &Common) {
    modifiers.width = common.width;
    modifiers.height = common.height;
    modifiers.opacity = common.opacity;
    modifiers.background = common.background.map(Brush::Solid);
    modifiers.foreground = common.foreground.map(Brush::Solid);
    if common.match_parent_width || common.fills_remaining {
        modifiers.horizontal_alignment = Some(HorizontalAlignment::Stretch);
    }
    if common.match_parent_height || common.fills_remaining {
        modifiers.vertical_alignment = Some(VerticalAlignment::Stretch);
    }
    let (top, left, bottom, right) = common.insets;
    if top != 0.0 || left != 0.0 || bottom != 0.0 || right != 0.0 {
        modifiers.padding = Some(Thickness {
            left,
            top,
            right,
            bottom,
        });
    }
    if common.on_click != 0 {
        let callback = common.on_click;
        modifiers
            .pointer_handlers
            .get_or_insert_with(Default::default)
            .on_tapped = Some(Callback::new(move |()| invoke0(callback)));
    }
}

fn render_children(handles: &[i64], detach_hidden: bool) -> Vec<Element> {
    handles
        .iter()
        .filter_map(|handle| {
            let n = node(*handle)?;
            if detach_hidden && n.common.hidden {
                None
            } else {
                Some(render_handle(*handle))
            }
        })
        .collect()
}

fn render_handle(handle: i64) -> Element {
    let Some(node) = node(handle) else {
        return Element::Empty;
    };
    if node.common.hidden {
        return Element::Empty;
    }
    let key = format!("perry-{handle}");
    let mut element: Element = match &node.kind {
        NodeKind::Text {
            value,
            font_size,
            font_weight,
            font_family,
            selectable: _,
            max_lines: _,
            truncation: _,
        } => {
            let mut view = TextBlock::new(value.clone());
            view.font_size = *font_size;
            view.font_weight = *font_weight;
            view.modifiers.font_family = font_family.clone();
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Button {
            label,
            callback,
            bordered,
        } => {
            let selected = if node.common.on_click != 0 {
                node.common.on_click
            } else {
                *callback
            };
            let mut view = fluent_button(label.clone()).on_click(move || invoke0(selected));
            view.style = if *bordered {
                ButtonStyle::Default
            } else {
                ButtonStyle::Subtle
            };
            view.is_enabled = node.common.enabled;
            apply_common(&mut view.modifiers, &node.common);
            // `Button::on_click` already owns the generic callback.
            view.modifiers.pointer_handlers = None;
            view.into()
        }
        NodeKind::VStack { spacing } | NodeKind::LazyVStack { spacing } => {
            let children = render_children(&node.common.children, node.common.detaches_hidden);
            let mut view = fluent_vstack(children).spacing(*spacing);
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::HStack { spacing } => {
            let children = render_children(&node.common.children, node.common.detaches_hidden);
            let mut view = fluent_hstack(children).spacing(*spacing);
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::ZStack => {
            let children = render_children(&node.common.children, node.common.detaches_hidden);
            let mut view = grid(children);
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Spacer => {
            let mut view = Border::default();
            view.modifiers.min_width = Some(8.0);
            view.modifiers.min_height = Some(8.0);
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Divider => {
            let mut view = Border::default();
            view.modifiers.height = Some(1.0);
            view.modifiers.horizontal_alignment = Some(HorizontalAlignment::Stretch);
            view.modifiers.background = Some(Brush::Solid(Color::rgb(128, 128, 128)));
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::TextField {
            value,
            placeholder,
            callback,
            borderless: _,
            font_size,
        } => {
            let cb = *callback;
            let mut view = TextBox::new(value.clone())
                .placeholder(placeholder.clone())
                .on_changed(move |value: String| {
                    set_textfield_value(handle, value.clone());
                    invoke_string(cb, &value);
                });
            view.is_enabled = node.common.enabled;
            view.modifiers.font_size = *font_size;
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::SecureField {
            value,
            placeholder,
            callback,
        } => {
            let cb = *callback;
            let mut view = PasswordBox::new()
                .value(value.clone())
                .placeholder(placeholder.clone())
                .on_changed(move |value: String| {
                    set_securefield_value(handle, value.clone());
                    invoke_string(cb, &value);
                });
            view.is_enabled = node.common.enabled;
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Toggle {
            label,
            on,
            callback,
        } => {
            let cb = *callback;
            let mut view = ToggleSwitch::new(*on)
                .header(label.clone())
                .on_changed(move |value| {
                    set_toggle_value(handle, value);
                    invoke1(cb, if value { 1.0 } else { 0.0 });
                });
            view.is_enabled = node.common.enabled;
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Slider {
            min,
            max,
            value,
            callback,
        } => {
            let cb = *callback;
            let mut view = Slider::new(*value)
                .range(*min, *max)
                .on_changed(move |value| {
                    set_slider_value(handle, value);
                    invoke1(cb, value);
                });
            view.is_enabled = node.common.enabled;
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::ScrollView { .. } => {
            let child = node
                .common
                .children
                .first()
                .copied()
                .map(render_handle)
                .unwrap_or(Element::Empty);
            let mut view = ScrollViewer::new(child);
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Form => {
            let children = render_children(&node.common.children, node.common.detaches_hidden);
            let mut view = fluent_vstack(children).spacing(12.0);
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Section { title } => {
            let mut children = vec![text_block(title.clone()).semibold().into()];
            children.extend(render_children(
                &node.common.children,
                node.common.detaches_hidden,
            ));
            let mut view = fluent_vstack(children).spacing(8.0);
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
        NodeKind::Progress { value } => {
            let mut view = if *value < 0.0 {
                ProgressBar::indeterminate()
            } else {
                ProgressBar::new(value.clamp(0.0, 1.0)).range(0.0, 1.0)
            };
            apply_common(&mut view.modifiers, &node.common);
            view.into()
        }
    };

    // Resource-backed border properties are available on the Border variants
    // used for layout primitives. Other controls retain their native Fluent
    // stroke and corner resources.
    if let Element::Border(view) = &mut element {
        view.corner_radius = node.common.corner_radius;
        view.border_thickness = node.common.border_width.map(Thickness::uniform);
        view.border_brush = node.common.border_color.map(Into::into);
    }
    element.with_key(key)
}

pub fn add_child(parent: i64, child: i64) {
    if !is_fluent() {
        perry_ui_windows::widgets::add_child(parent, child);
        return;
    }
    with_node_mut(parent, |node| node.common.children.push(child));
}

pub fn add_child_at(parent: i64, child: i64, index: i64) {
    if !is_fluent() {
        perry_ui_windows::widgets::add_child_at(parent, child, index);
        return;
    }
    with_node_mut(parent, |node| {
        let index = index.max(0) as usize;
        let index = index.min(node.common.children.len());
        node.common.children.insert(index, child);
    });
}

pub fn remove_child(parent: i64, child: i64) {
    if !is_fluent() {
        perry_ui_windows::widgets::remove_child(parent, child);
        return;
    }
    with_node_mut(parent, |node| node.common.children.retain(|h| *h != child));
}

pub fn clear_children(handle: i64) {
    if !is_fluent() {
        perry_ui_windows::widgets::clear_children(handle);
        return;
    }
    with_node_mut(handle, |node| node.common.children.clear());
}

pub fn set_fixed_width(handle: i64, width: i32) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_fixed_width(handle, width);
        return;
    }
    with_node_mut(handle, |node| node.common.width = Some(width as f64));
}

pub fn set_fixed_height(handle: i64, height: i32) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_fixed_height(handle, height);
        return;
    }
    with_node_mut(handle, |node| node.common.height = Some(height as f64));
}

pub fn set_match_parent_width(handle: i64, value: bool) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_match_parent_width(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.match_parent_width = value);
}

pub fn set_match_parent_height(handle: i64, value: bool) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_match_parent_height(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.match_parent_height = value);
}

pub fn set_fills_remaining(handle: i64, value: bool) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_fills_remaining(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.fills_remaining = value);
}

pub fn set_hugging_priority(handle: i64, priority: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_hugging_priority(handle, priority);
        return;
    }
    if priority <= 250.0 {
        set_fills_remaining(handle, true);
    }
}

pub fn set_hidden(handle: i64, value: bool) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_hidden(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.hidden = value);
}

pub fn set_enabled(handle: i64, value: bool) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_enabled(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.enabled = value);
}

pub fn set_detaches_hidden(handle: i64, value: bool) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_detaches_hidden(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.detaches_hidden = value);
}

pub fn set_distribution(handle: i64, value: i64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_distribution(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.distribution = value);
}

pub fn set_alignment(handle: i64, value: i64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_alignment(handle, value);
        return;
    }
    with_node_mut(handle, |node| node.common.alignment = value);
}

pub fn set_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_insets(handle, top, left, bottom, right);
        return;
    }
    with_node_mut(handle, |node| {
        node.common.insets = (top, left, bottom, right)
    });
}

pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_background_color(handle, r, g, b, a);
        return;
    }
    with_node_mut(handle, |node| {
        node.common.background = Some(color(r, g, b, a))
    });
}

#[allow(clippy::too_many_arguments)]
pub fn set_background_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    _r2: f64,
    _g2: f64,
    _b2: f64,
    _a2: f64,
    _direction: f64,
) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_background_gradient(
            handle, r1, g1, b1, a1, _r2, _g2, _b2, _a2, _direction,
        );
        return;
    }
    // Reactor's WinUI backend supports Composition animation; Perry's public
    // two-stop gradient remains represented by its first color until the
    // cross-platform style ABI exposes gradient stops as a typed collection.
    set_background_color(handle, r1, g1, b1, a1);
}

pub fn set_opacity(handle: i64, value: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_opacity(handle, value);
        return;
    }
    with_node_mut(handle, |node| {
        node.common.opacity = Some(value.clamp(0.0, 1.0))
    });
}

pub fn set_corner_radius(handle: i64, value: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_corner_radius(handle, value);
        return;
    }
    with_node_mut(handle, |node| {
        node.common.corner_radius = Some(value.max(0.0))
    });
}

pub fn set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_border_color(handle, r, g, b, a);
        return;
    }
    with_node_mut(handle, |node| {
        node.common.border_color = Some(color(r, g, b, a))
    });
}

pub fn set_border_width(handle: i64, value: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_border_width(handle, value);
        return;
    }
    with_node_mut(handle, |node| {
        node.common.border_width = Some(value.max(0.0))
    });
}

#[allow(clippy::too_many_arguments)]
pub fn set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_shadow(handle, r, g, b, a, blur, offset_x, offset_y);
    }
}

pub fn set_tooltip(handle: i64, text_ptr: *const u8) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_tooltip(handle, text_ptr);
        return;
    }
    let value = read_string(text_ptr);
    with_node_mut(handle, |node| node.common.tooltip = Some(value));
}

pub fn set_control_size(handle: i64, size: i64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_control_size(handle, size);
    }
}

pub fn set_on_click(handle: i64, callback: f64) {
    let callback = callback_ptr(callback);
    with_node_mut(handle, |node| node.common.on_click = callback);
}

pub fn set_on_hover(handle: i64, callback: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_on_hover(handle, callback);
    }
}

pub fn set_on_double_click(handle: i64, callback: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::set_on_double_click(handle, callback);
    }
}

pub fn animate_opacity(handle: i64, target: f64, _duration: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::animate_opacity(handle, target, _duration);
        return;
    }
    set_opacity(handle, target);
}

pub fn animate_position(handle: i64, dx: f64, dy: f64, duration: f64) {
    if !is_fluent() {
        perry_ui_windows::widgets::animate_position(handle, dx, dy, duration);
    }
}

pub fn get_hwnd(handle: i64) -> Option<HWND> {
    if is_fluent() {
        None
    } else {
        perry_ui_windows::widgets::get_hwnd(handle)
    }
}

pub fn register_widget(hwnd: HWND, kind: WidgetKind, control_id: u16) -> i64 {
    if is_fluent() {
        0
    } else {
        perry_ui_windows::widgets::register_widget(hwnd, kind, control_id)
    }
}

fn set_textfield_value(handle: i64, value: String) {
    with_node_mut(handle, |node| {
        if let NodeKind::TextField { value: current, .. } = &mut node.kind {
            *current = value;
        }
    });
}

fn set_securefield_value(handle: i64, value: String) {
    with_node_mut(handle, |node| {
        if let NodeKind::SecureField { value: current, .. } = &mut node.kind {
            *current = value;
        }
    });
}

fn set_toggle_value(handle: i64, value: bool) {
    with_node_mut(handle, |node| {
        if let NodeKind::Toggle { on, .. } = &mut node.kind {
            *on = value;
        }
    });
}

fn set_slider_value(handle: i64, value: f64) {
    with_node_mut(handle, |node| {
        if let NodeKind::Slider { value: current, .. } = &mut node.kind {
            *current = value;
        }
    });
}

pub mod text {
    use super::*;

    pub fn create(text_ptr: *const u8) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::text::create(text_ptr);
        }
        register(NodeKind::Text {
            value: read_string(text_ptr),
            font_size: None,
            font_weight: None,
            font_family: None,
            selectable: false,
            max_lines: None,
            truncation: false,
        })
    }

    pub fn set_string(handle: i64, text_ptr: *const u8) {
        set_text_str(handle, &read_string(text_ptr));
    }

    pub fn set_text_str(handle: i64, value: &str) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_text_str(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Text { value: current, .. } = &mut node.kind {
                *current = value.to_owned();
            }
        });
    }

    pub fn set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_color(handle, r, g, b, a);
            return;
        }
        with_node_mut(handle, |node| {
            node.common.foreground = Some(color(r, g, b, a))
        });
    }

    pub fn set_font_size(handle: i64, size: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_font_size(handle, size);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Text { font_size, .. } = &mut node.kind {
                *font_size = Some(size);
            }
        });
    }

    pub fn set_font_weight(handle: i64, _size: f64, weight: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_font_weight(handle, _size, weight);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Text { font_weight, .. } = &mut node.kind {
                *font_weight = Some(weight.clamp(1.0, 1000.0) as u16);
            }
        });
    }

    pub fn set_selectable(handle: i64, value: bool) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_selectable(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Text { selectable, .. } = &mut node.kind {
                *selectable = value;
            }
        });
    }

    pub fn set_number_of_lines(handle: i64, value: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_number_of_lines(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Text { max_lines, .. } = &mut node.kind {
                *max_lines = i32::try_from(value).ok();
            }
        });
    }

    pub fn set_truncation_mode(handle: i64, _mode: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_truncation_mode(handle, _mode);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Text { truncation, .. } = &mut node.kind {
                *truncation = true;
            }
        });
    }

    pub fn set_font_family(handle: i64, family_ptr: *const u8) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_font_family(handle, family_ptr);
            return;
        }
        let family = read_string(family_ptr);
        with_node_mut(handle, |node| {
            if let NodeKind::Text { font_family, .. } = &mut node.kind {
                *font_family = Some(family);
            }
        });
    }

    pub fn set_decoration(handle: i64, decoration: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_decoration(handle, decoration);
        }
    }
    pub fn set_text_alignment(handle: i64, alignment: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::text::set_text_alignment(handle, alignment);
        }
    }
}

pub mod button {
    use super::*;

    pub fn create(label_ptr: *const u8, callback: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::button::create(label_ptr, callback);
        }
        register(NodeKind::Button {
            label: read_string(label_ptr),
            callback: callback_ptr(callback),
            bordered: true,
        })
    }

    pub fn set_title(handle: i64, title_ptr: *const u8) {
        if !is_fluent() {
            perry_ui_windows::widgets::button::set_title(handle, title_ptr);
            return;
        }
        let title = read_string(title_ptr);
        with_node_mut(handle, |node| {
            if let NodeKind::Button { label, .. } = &mut node.kind {
                *label = title;
            }
        });
    }

    pub fn set_bordered(handle: i64, value: bool) {
        if !is_fluent() {
            perry_ui_windows::widgets::button::set_bordered(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Button { bordered, .. } = &mut node.kind {
                *bordered = value;
            }
        });
    }

    pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::button::set_text_color(handle, r, g, b, a);
            return;
        }
        with_node_mut(handle, |node| {
            node.common.foreground = Some(color(r, g, b, a))
        });
    }

    pub fn set_image(handle: i64, name_ptr: *const u8) {
        if !is_fluent() {
            perry_ui_windows::widgets::button::set_image(handle, name_ptr);
        }
    }
    pub fn set_image_position(handle: i64, position: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::button::set_image_position(handle, position);
        }
    }
}

pub mod vstack {
    use super::*;
    pub fn create(spacing: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::vstack::create(spacing);
        }
        register(NodeKind::VStack { spacing })
    }
    pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
        let handle = create(spacing);
        set_insets(handle, top, left, bottom, right);
        handle
    }
}

pub mod hstack {
    use super::*;
    pub fn create(spacing: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::hstack::create(spacing);
        }
        register(NodeKind::HStack { spacing })
    }
    pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
        let handle = create(spacing);
        set_insets(handle, top, left, bottom, right);
        handle
    }
}

pub mod zstack {
    use super::*;
    pub fn create() -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::zstack::create();
        }
        register(NodeKind::ZStack)
    }
}

pub mod spacer {
    use super::*;
    pub fn create() -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::spacer::create();
        }
        let handle = register(NodeKind::Spacer);
        set_fills_remaining(handle, true);
        handle
    }
}

pub mod divider {
    use super::*;
    pub fn create() -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::divider::create();
        }
        register(NodeKind::Divider)
    }
}

pub mod textfield {
    use super::*;

    pub fn create(placeholder_ptr: *const u8, callback: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::textfield::create(placeholder_ptr, callback);
        }
        register(NodeKind::TextField {
            value: String::new(),
            placeholder: read_string(placeholder_ptr),
            callback: callback_ptr(callback),
            borderless: false,
            font_size: None,
        })
    }

    pub fn set_string_value(handle: i64, value_ptr: *const u8) {
        set_string_str(handle, &read_string(value_ptr));
    }

    pub fn set_string_str(handle: i64, value: &str) {
        if !is_fluent() {
            let bytes = value.as_bytes();
            unsafe {
                let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                perry_ui_windows::widgets::textfield::set_string_value(handle, ptr);
            }
            return;
        }
        set_textfield_value(handle, value.to_owned());
    }

    pub fn get_string(handle: i64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::textfield::get_string(handle);
        }
        let value = node(handle)
            .and_then(|node| match node.kind {
                NodeKind::TextField { value, .. } => Some(value),
                _ => None,
            })
            .unwrap_or_default();
        unsafe { js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64 }
    }

    pub fn set_borderless(handle: i64, value: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::textfield::set_borderless(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::TextField { borderless, .. } = &mut node.kind {
                *borderless = value > 0.5;
            }
        });
    }

    pub fn set_font_size(handle: i64, value: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::textfield::set_font_size(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::TextField { font_size, .. } = &mut node.kind {
                *font_size = Some(value);
            }
        });
    }

    pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
        super::set_background_color(handle, r, g, b, a);
    }
    pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::textfield::set_text_color(handle, r, g, b, a);
            return;
        }
        with_node_mut(handle, |node| {
            node.common.foreground = Some(color(r, g, b, a))
        });
    }
    pub fn focus(handle: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::textfield::focus(handle);
        }
    }
}

pub mod securefield {
    use super::*;
    pub fn create(placeholder_ptr: *const u8, callback: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::securefield::create(placeholder_ptr, callback);
        }
        register(NodeKind::SecureField {
            value: String::new(),
            placeholder: read_string(placeholder_ptr),
            callback: callback_ptr(callback),
        })
    }
}

pub mod toggle {
    use super::*;
    pub fn create(label_ptr: *const u8, callback: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::toggle::create(label_ptr, callback);
        }
        register(NodeKind::Toggle {
            label: read_string(label_ptr),
            on: false,
            callback: callback_ptr(callback),
        })
    }
    pub fn set_state(handle: i64, value: i32) {
        if !is_fluent() {
            perry_ui_windows::widgets::toggle::set_state(handle, value);
            return;
        }
        set_toggle_value(handle, value != 0);
    }
}

pub mod slider {
    use super::*;
    pub fn create(min: f64, max: f64, value: f64, callback: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::slider::create(min, max, value, callback);
        }
        register(NodeKind::Slider {
            min,
            max,
            value,
            callback: callback_ptr(callback),
        })
    }
    pub fn set_value(handle: i64, value: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::slider::set_value(handle, value);
            return;
        }
        set_slider_value(handle, value);
    }
    pub fn get_value(handle: i64) -> Option<f64> {
        if !is_fluent() {
            return perry_ui_windows::widgets::slider::get_value(handle);
        }
        node(handle).and_then(|node| match node.kind {
            NodeKind::Slider { value, .. } => Some(value),
            _ => None,
        })
    }
}

pub mod scrollview {
    use super::*;
    pub fn create() -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::scrollview::create();
        }
        register(NodeKind::ScrollView { offset: 0.0 })
    }
    pub fn set_child(handle: i64, child: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::scrollview::set_child(handle, child);
            return;
        }
        with_node_mut(handle, |node| node.common.children = vec![child]);
    }
    pub fn set_offset(handle: i64, value: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::scrollview::set_offset(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::ScrollView { offset } = &mut node.kind {
                *offset = value;
            }
        });
    }
    pub fn get_offset(handle: i64) -> f64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::scrollview::get_offset(handle);
        }
        node(handle)
            .and_then(|node| match node.kind {
                NodeKind::ScrollView { offset } => Some(offset),
                _ => None,
            })
            .unwrap_or(0.0)
    }
    pub fn set_scroll_end_callback(handle: i64, callback: f64, threshold_px: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::scrollview::set_scroll_end_callback(
                handle,
                callback,
                threshold_px,
            );
        }
    }
}

pub mod form {
    use super::*;
    pub fn create() -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::form::create();
        }
        register(NodeKind::Form)
    }
    pub fn section_create(title_ptr: *const u8) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::form::section_create(title_ptr);
        }
        register(NodeKind::Section {
            title: read_string(title_ptr),
        })
    }
}

pub mod lazyvstack {
    use super::*;
    pub fn create(_count: f64, _render: f64) -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::lazyvstack::create(_count, _render);
        }
        register(NodeKind::LazyVStack { spacing: 4.0 })
    }
    pub fn update(_handle: i64, _count: i64) {
        if !is_fluent() {
            perry_ui_windows::widgets::lazyvstack::update(_handle, _count);
        }
    }
}

pub mod progressview {
    use super::*;
    pub fn create() -> i64 {
        if !is_fluent() {
            return perry_ui_windows::widgets::progressview::create();
        }
        register(NodeKind::Progress { value: -1.0 })
    }
    pub fn set_value(handle: i64, value: f64) {
        if !is_fluent() {
            perry_ui_windows::widgets::progressview::set_value(handle, value);
            return;
        }
        with_node_mut(handle, |node| {
            if let NodeKind::Progress { value: current } = &mut node.kind {
                *current = value;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fluent_model_preserves_tree_order_and_properties() {
        let parent = register(NodeKind::VStack { spacing: 12.0 });
        let first = register(NodeKind::Text {
            value: "first".into(),
            font_size: None,
            font_weight: None,
            font_family: None,
            selectable: false,
            max_lines: None,
            truncation: false,
        });
        let second = register(NodeKind::Divider);
        with_node_mut(parent, |node| {
            node.common.children.push(first);
            node.common.children.push(second);
        });
        let node = node(parent).unwrap();
        assert_eq!(node.common.children, vec![first, second]);
        assert_eq!(render_handle(parent).kind_name(), "StackPanel");
    }
}
