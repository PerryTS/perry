// Perry Web Runtime - maps perry/ui widgets to DOM elements
// This file is embedded via include_str! and injected into HTML output.

(function() {
"use strict";

// --- Handle System ---
// Widget handles are wrapper objects with methods that delegate to DOM elements.
// State handles are objects with .value getter/setter and methods.

const handles = new Map();   // handle int → DOM element
const states = new Map();    // handle int → { _value, subscribers[] }
let nextHandle = 1;
const perryImageCache = new Map();

function allocHandle(el) {
    const h = nextHandle++;
    handles.set(h, el);
    return h;
}

function getHandle(h) {
    if (typeof h === "object" && h !== null && h._perryHandle) return handles.get(h._perryHandle);
    return handles.get(h);
}

function getHandleId(h) {
    if (typeof h === "object" && h !== null && h._perryHandle) return h._perryHandle;
    return h;
}

// Create a widget wrapper object with all perry/ui methods
function wrapWidget(h) {
    const w = {
        _perryHandle: h,
        addChild(child) { perry_ui_widget_add_child(h, getHandleId(child)); },
        removeAllChildren() { perry_ui_widget_remove_all_children(h); },
        setBackground(r, g, b, a) { perry_ui_set_background(h, r, g, b, a); },
        setForeground(r, g, b, a) { perry_ui_set_foreground(h, r, g, b, a); },
        setFontSize(size) { perry_ui_set_font_size(h, size); },
        setFontWeight(weight) { perry_ui_set_font_weight(h, weight); },
        setFontFamily(family) { perry_ui_set_font_family(h, family); },
        setPadding(val) { perry_ui_set_padding(h, val); },
        setFrame(w, ht) { perry_ui_set_frame(h, w, ht); },
        setCornerRadius(r) { perry_ui_set_corner_radius(h, r); },
        setBorder(w, r, g, b, a) { perry_ui_set_border(h, w, r, g, b, a); },
        setOpacity(o) { perry_ui_set_opacity(h, o); },
        setEnabled(e) { perry_ui_set_enabled(h, e); },
        setTooltip(t) { perry_ui_set_tooltip(h, t); },
        setControlSize(s) { perry_ui_set_control_size(h, s); },
        animateOpacity(target, durationSecs) { perry_ui_animate_opacity(h, target, durationSecs); },
        animatePosition(dx, dy, durationSecs) { perry_ui_animate_position(h, dx, dy, durationSecs); },
        setOnClick(cb) { perry_ui_set_on_click(h, cb); },
        setOnHover(cb) { perry_ui_set_on_hover(h, cb); },
        setOnDoubleClick(cb) { perry_ui_set_on_double_click(h, cb); },
        run() { perry_ui_app_run(); },
        // Canvas methods
        fillRect(x, y, w, ht) { perry_ui_canvas_fill_rect(h, x, y, w, ht); },
        strokeRect(x, y, w, ht) { perry_ui_canvas_stroke_rect(h, x, y, w, ht); },
        clearRect(x, y, w, ht) { perry_ui_canvas_clear_rect(h, x, y, w, ht); },
        setFillColor(r, g, b, a) { perry_ui_canvas_set_fill_color(h, r, g, b, a); },
        setStrokeColor(r, g, b, a) { perry_ui_canvas_set_stroke_color(h, r, g, b, a); },
        beginPath() { perry_ui_canvas_begin_path(h); },
        moveTo(x, y) { perry_ui_canvas_move_to(h, x, y); },
        lineTo(x, y) { perry_ui_canvas_line_to(h, x, y); },
        arc(x, y, r, sa, ea) { perry_ui_canvas_arc(h, x, y, r, sa, ea); },
        closePath() { perry_ui_canvas_close_path(h); },
        fill() { perry_ui_canvas_fill(h); },
        stroke() { perry_ui_canvas_stroke(h); },
        setLineWidth(w) { perry_ui_canvas_set_line_width(h, w); },
        fillText(t, x, y) { perry_ui_canvas_fill_text(h, t, x, y); },
        setFont(f) { perry_ui_canvas_set_font(h, f); },
        drawImage(image, ...args) { perry_ui_canvas_draw_image(h, image, ...args); },
    };
    return w;
}

// --- State Reactive System ---
function stateCreate(initialValue) {
    const h = nextHandle++;
    const sObj = { _value: initialValue, subscribers: [] };
    states.set(h, sObj);
    // Return a state wrapper with .value getter/setter and methods
    const wrapper = {
        _perryHandle: h,
        _perryState: true,
        get value() { return sObj._value; },
        set value(v) { stateSet(h, v); },
        get() { return sObj._value; },
        set(v) { stateSet(h, v); },
        bindText(widget) { perry_ui_state_bind_text(h, getHandleId(widget)); },
        bindTextNumeric(widget) { perry_ui_state_bind_text_numeric(h, getHandleId(widget)); },
        bindSlider(widget) { perry_ui_state_bind_slider(h, getHandleId(widget)); },
        bindToggle(widget) { perry_ui_state_bind_toggle(h, getHandleId(widget)); },
        bindVisibility(widget) { perry_ui_state_bind_visibility(h, getHandleId(widget)); },
        bindForEach(parent, fn) { perry_ui_state_bind_foreach(h, getHandleId(parent), fn); },
        onChange(cb) { perry_ui_state_on_change(h, cb); },
    };
    return wrapper;
}

function stateGet(h) {
    const hId = getHandleId(h);
    const s = states.get(hId);
    return s ? s._value : undefined;
}

function stateSet(h, value) {
    const hId = getHandleId(h);
    const s = states.get(hId);
    if (!s) return;
    s._value = value;
    for (const sub of s.subscribers) {
        try { sub(value); } catch(e) { console.error("State subscriber error:", e); }
    }
}

function stateSubscribe(h, fn) {
    const hId = getHandleId(h);
    const s = states.get(hId);
    if (s) s.subscribers.push(fn);
}

// --- CSS Reset ---
const style = document.createElement("style");
style.textContent = `
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
html, body { width: 100vw; height: 100vh; overflow: hidden;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
#perry-root { display: flex; flex-direction: column; width: 100%; flex: 1 1 0%; min-height: 0; overflow: hidden; }
button { cursor: pointer; padding: 6px 16px; border: 1px solid #ccc; border-radius: 6px; background: transparent; font: inherit; color: inherit; }
button:hover { opacity: 0.85; }
button:active { opacity: 0.7; }
input[type="text"], input[type="password"], select, textarea { padding: 6px 10px; border: 1px solid #ccc; border-radius: 6px; font: inherit; }
input[type="range"] { width: 100%; }
hr { border: none; border-top: 1px solid #ddd; margin: 4px 0; }
fieldset { border: 1px solid #ddd; border-radius: 8px; padding: 12px; }
legend { font-weight: 600; padding: 0 6px; }
progress { width: 100%; }
`;
document.head.appendChild(style);

// --- Root ---
let perryRoot = null;
function getRoot() {
    if (!perryRoot) {
        perryRoot = document.getElementById("perry-root");
        if (!perryRoot) {
            perryRoot = document.createElement("div");
            perryRoot.id = "perry-root";
            document.body.appendChild(perryRoot);
        }
    }
    return perryRoot;
}

// --- Widget Creation ---
function perry_ui_app_create(titleOrOpts, width, height) {
    let title, bodyHandle;
    if (typeof titleOrOpts === "object" && titleOrOpts !== null) {
        // Called as App({title, width, height, body})
        title = titleOrOpts.title || "Perry App";
        bodyHandle = titleOrOpts.body;
    } else {
        title = titleOrOpts;
    }
    document.title = title;
    const root = getRoot();
    root.style.width = "100%";
    root.style.minHeight = "0";
    root.style.display = "flex";
    root.style.flexDirection = "column";
    root.style.overflow = "hidden";
    // Append body widget to root if provided
    if (bodyHandle != null) {
        const h = (typeof bodyHandle === "object" && bodyHandle._perryHandle) ? bodyHandle._perryHandle : bodyHandle;
        const bodyEl = handles.get(h);
        if (bodyEl) {
            bodyEl.style.flex = "1 1 0%";
            root.appendChild(bodyEl);
        }
    }
    return wrapWidget(allocHandle(root));
}

function _appendChildren(el, children) {
    if (!children || !Array.isArray(children)) return;
    for (const child of children) {
        const ch = (typeof child === "object" && child !== null && child._perryHandle)
            ? child._perryHandle : child;
        const childEl = handles.get(ch);
        if (childEl) el.appendChild(childEl);
    }
}

function perry_ui_vstack_create(spacing, children) {
    const el = document.createElement("div");
    el.style.display = "flex";
    el.style.flexDirection = "column";
    el.style.gap = spacing + "px";
    const h = allocHandle(el);
    _appendChildren(el, children);
    return wrapWidget(h);
}

function perry_ui_hstack_create(spacing, children) {
    const el = document.createElement("div");
    el.style.display = "flex";
    el.style.flexDirection = "row";
    el.style.gap = spacing + "px";
    el.style.alignItems = "stretch";
    const h = allocHandle(el);
    _appendChildren(el, children);
    return wrapWidget(h);
}

function perry_ui_zstack_create(children) {
    const el = document.createElement("div");
    el.style.position = "relative";
    const h = allocHandle(el);
    _appendChildren(el, children);
    return wrapWidget(h);
}

function perry_ui_text_create(text, id) {
    const el = document.createElement("span");
    el.textContent = (text !== undefined && text !== null) ? String(text) : "";
    const h = allocHandle(el);
    // Issue #1392 — 2-arg `Text(content, id)` form. state_desugar emits
    // `count.text()` as `Text("<initial>", "__state_N")`, reusing the synth
    // id as both state key and setText id so a write fans out to the widget.
    if (typeof id === "string" && id.length > 0) uiTextRegister(h, id);
    return wrapWidget(h);
}

function perry_ui_button_create(label, callback) {
    const el = document.createElement("button");
    el.textContent = label;
    if (typeof callback === "function") {
        el.addEventListener("click", callback);
    }
    return wrapWidget(allocHandle(el));
}

function perry_ui_textfield_create(placeholder, callback) {
    const el = document.createElement("input");
    el.type = "text";
    el.placeholder = placeholder || "";
    if (typeof callback === "function") {
        el.addEventListener("input", () => callback(el.value));
    }
    return wrapWidget(allocHandle(el));
}

function perry_ui_securefield_create(placeholder, callback) {
    const el = document.createElement("input");
    el.type = "password";
    el.placeholder = placeholder || "";
    if (typeof callback === "function") {
        el.addEventListener("input", () => callback(el.value));
    }
    return wrapWidget(allocHandle(el));
}

function perry_ui_toggle_create(label, callback) {
    const wrapper = document.createElement("label");
    wrapper.style.display = "flex";
    wrapper.style.alignItems = "center";
    wrapper.style.gap = "8px";
    wrapper.style.cursor = "pointer";
    const input = document.createElement("input");
    input.type = "checkbox";
    wrapper.appendChild(input);
    wrapper.appendChild(document.createTextNode(label || ""));
    if (typeof callback === "function") {
        input.addEventListener("change", () => callback(input.checked ? 1.0 : 0.0));
    }
    wrapper._input = input;
    return wrapWidget(allocHandle(wrapper));
}

function perry_ui_slider_create(min, max, callback) {
    const el = document.createElement("input");
    el.type = "range";
    el.min = min;
    el.max = max;
    el.value = min; // codegen emits 3-arg Slider(min, max, onChange); default initial=min
    el.step = "any";
    if (typeof callback === "function") {
        el.addEventListener("input", () => callback(parseFloat(el.value)));
    }
    return wrapWidget(allocHandle(el));
}

function perry_ui_scrollview_create() {
    const el = document.createElement("div");
    el.style.overflow = "auto";
    el.style.flex = "1";
    return wrapWidget(allocHandle(el));
}

function perry_ui_spacer_create() {
    const el = document.createElement("div");
    el.style.flex = "1";
    return wrapWidget(allocHandle(el));
}

function perry_ui_divider_create() {
    const el = document.createElement("hr");
    return wrapWidget(allocHandle(el));
}

function perry_ui_progressview_create(value) {
    const el = document.createElement("progress");
    el.max = 1;
    el.value = (value != null) ? value : 0;
    return wrapWidget(allocHandle(el));
}

function perry_ui_image_create(src, width, height) {
    const el = document.createElement("img");
    el.src = src || "";
    if (width > 0) el.style.width = width + "px";
    if (height > 0) el.style.height = height + "px";
    el.style.objectFit = "contain";
    return wrapWidget(allocHandle(el));
}

function perry_ui_picker_create(items_json, selected, callback) {
    const el = document.createElement("select");
    let items = [];
    try { items = JSON.parse(items_json); } catch(e) {}
    for (let i = 0; i < items.length; i++) {
        const opt = document.createElement("option");
        opt.value = i;
        opt.textContent = items[i];
        if (i === selected) opt.selected = true;
        el.appendChild(opt);
    }
    if (typeof callback === "function") {
        el.addEventListener("change", () => callback(parseInt(el.value)));
    }
    return wrapWidget(allocHandle(el));
}

function perry_ui_form_create() {
    const el = document.createElement("form");
    el.addEventListener("submit", e => e.preventDefault());
    el.style.display = "flex";
    el.style.flexDirection = "column";
    el.style.gap = "8px";
    return wrapWidget(allocHandle(el));
}

function perry_ui_section_create(title) {
    const el = document.createElement("fieldset");
    if (title) {
        const legend = document.createElement("legend");
        legend.textContent = title;
        el.appendChild(legend);
    }
    el.style.display = "flex";
    el.style.flexDirection = "column";
    el.style.gap = "6px";
    return wrapWidget(allocHandle(el));
}

function perry_ui_navigationstack_create() {
    const el = document.createElement("div");
    el._navStack = [];
    return wrapWidget(allocHandle(el));
}

function perry_ui_canvas_create(width, height) {
    const el = document.createElement("canvas");
    el.width = width;
    el.height = height;
    el._ctx = el.getContext("2d");
    return wrapWidget(allocHandle(el));
}

// --- Child Management ---
function perry_ui_widget_add_child(parent_h, child_h) {
    const parent = getHandle(parent_h);
    const child = getHandle(child_h);
    if (parent && child) parent.appendChild(child);
}

function perry_ui_widget_remove_all_children(h) {
    const el = getHandle(h);
    if (el) {
        while (el.lastChild && el.lastChild.tagName !== "LEGEND") {
            el.removeChild(el.lastChild);
        }
    }
}

// Resolve handle-or-wrapper to int for internal use
function resolveHandle(h) {
    if (typeof h === "object" && h !== null && h._perryHandle) return h._perryHandle;
    return h;
}

// --- Styling ---
function perry_ui_set_background(h, r, g, b, a) {
    const el = getHandle(h);
    if (el) el.style.backgroundColor = `rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

function perry_ui_set_foreground(h, r, g, b, a) {
    const el = getHandle(h);
    if (el) el.style.color = `rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

function perry_ui_set_font_size(h, size) {
    const el = getHandle(h);
    if (el) el.style.fontSize = size + "px";
}

function perry_ui_set_font_weight(h, weight) {
    const el = getHandle(h);
    if (el) el.style.fontWeight = weight === 1 ? "bold" : "normal";
}

function perry_ui_set_font_family(h, family) {
    const el = getHandle(h);
    if (el) el.style.fontFamily = family;
}

function perry_ui_set_padding(h, value) {
    const el = getHandle(h);
    if (el) el.style.padding = value + "px";
}

function perry_ui_set_frame(h, width, height) {
    const el = getHandle(h);
    if (el) {
        if (width > 0) el.style.width = width + "px";
        if (height > 0) el.style.height = height + "px";
    }
}

function perry_ui_set_corner_radius(h, radius) {
    const el = getHandle(h);
    if (el) el.style.borderRadius = radius + "px";
}

function perry_ui_set_border(h, width, r, g, b, a) {
    const el = getHandle(h);
    if (el) el.style.border = `${width}px solid rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

function perry_ui_set_opacity(h, opacity) {
    const el = getHandle(h);
    if (el) el.style.opacity = opacity;
}

function perry_ui_set_enabled(h, enabled) {
    const el = getHandle(h);
    if (el) {
        el.disabled = !enabled;
        el.style.opacity = enabled ? "1" : "0.5";
        el.style.pointerEvents = enabled ? "auto" : "none";
    }
}

function perry_ui_set_tooltip(h, text) {
    const el = getHandle(h);
    if (el) el.title = text;
}

function perry_ui_set_control_size(h, size) {
    const el = getHandle(h);
    if (!el) return;
    const scale = size === 0 ? 0.85 : size === 2 ? 1.2 : 1.0;
    el.style.fontSize = (scale * 100) + "%";
}

// --- Animations ---
// Canonical API: animateOpacity(target, durationSecs).
// The animation runs from the element's *current* opacity to `target`.
function perry_ui_animate_opacity(h, target, durationSecs) {
    const el = getHandle(h);
    if (!el) return;
    const cur = el.style.opacity === "" ? "1" : el.style.opacity;
    el.style.opacity = cur;
    el.style.transition = `opacity ${durationSecs}s ease`;
    requestAnimationFrame(() => { el.style.opacity = target; });
}

// animatePosition(dx, dy, durationSecs) — delta relative to current position.
function perry_ui_animate_position(h, dx, dy, durationSecs) {
    const el = getHandle(h);
    if (!el) return;
    if (!el.style.position || el.style.position === "static") el.style.position = "relative";
    const curLeft = parseFloat(el.style.left) || 0;
    const curTop = parseFloat(el.style.top) || 0;
    el.style.transition = `left ${durationSecs}s ease, top ${durationSecs}s ease`;
    requestAnimationFrame(() => {
        el.style.left = (curLeft + dx) + "px";
        el.style.top = (curTop + dy) + "px";
    });
}

// --- Event Handlers ---
function perry_ui_set_on_click(h, callback) {
    const el = getHandle(h);
    if (el && typeof callback === "function") el.addEventListener("click", callback);
}

function perry_ui_set_on_hover(h, callback) {
    const el = getHandle(h);
    if (!el || typeof callback !== "function") return;
    el.addEventListener("mouseenter", () => callback(1));
    el.addEventListener("mouseleave", () => callback(0));
}

function perry_ui_set_on_double_click(h, callback) {
    const el = getHandle(h);
    if (el && typeof callback === "function") el.addEventListener("dblclick", callback);
}

// --- State Bindings ---
function perry_ui_state_bind_text(stateH, widgetH) {
    const el = getHandle(widgetH);
    if (!el) return;
    stateSubscribe(stateH, (v) => { el.textContent = String(v); });
    el.textContent = String(stateGet(stateH));
}

function perry_ui_state_bind_text_numeric(stateH, widgetH) {
    perry_ui_state_bind_text(stateH, widgetH);
}

function perry_ui_state_bind_slider(stateH, widgetH) {
    const el = getHandle(widgetH);
    if (!el) return;
    stateSubscribe(stateH, (v) => { el.value = v; });
    el.value = stateGet(stateH);
}

function perry_ui_state_bind_toggle(stateH, widgetH) {
    const el = getHandle(widgetH);
    if (!el) return;
    const input = el._input || el.querySelector("input[type=checkbox]");
    if (!input) return;
    stateSubscribe(stateH, (v) => { input.checked = !!v; });
    input.checked = !!stateGet(stateH);
}

// Issue #5076: programmatically set a Toggle's on/off state. `on` is
// 0 for off, non-zero for on.
function perry_ui_toggle_set_state(widgetH, on) {
    const el = getHandle(widgetH);
    if (!el) return;
    const input = el._input || el.querySelector("input[type=checkbox]");
    if (input) input.checked = !!on;
}

function perry_ui_state_bind_visibility(stateH, widgetH) {
    const el = getHandle(widgetH);
    if (!el) return;
    function update(v) { el.style.display = v ? "" : "none"; }
    stateSubscribe(stateH, update);
    update(stateGet(stateH));
}

function perry_ui_state_bind_foreach(stateH, parentH, templateFn) {
    const parent = getHandle(parentH);
    if (!parent || typeof templateFn !== "function") return;
    function update(items) {
        perry_ui_widget_remove_all_children(parentH);
        if (Array.isArray(items)) {
            for (let i = 0; i < items.length; i++) {
                templateFn(items[i], i);
            }
        }
    }
    stateSubscribe(stateH, update);
    update(stateGet(stateH));
}

function perry_ui_state_on_change(stateH, callback) {
    if (typeof callback === "function") {
        stateSubscribe(stateH, callback);
    }
}

// --- Issue #1392: keyed state + setText registry ---
// The state_desugar pass (crates/perry-transform/src/state_desugar.rs) lowers
// `state<T>` to a synthetic, string-keyed registry API rather than the
// handle-based `perry_ui_state_create` surface above:
//   let c = state(0)   -> __state_init("__state_0", 0)
//   c.value / c.get()  -> __state_get("__state_0")
//   c.set(v)           -> __state_set("__state_0", v)
//   c.text()           -> Text("0", "__state_0")   (registers via uiTextRegister)
// The synth id is reused as both the state key and the Text widget's setText
// id, so a write fans out to every bound widget. Mirrors the native runtime
// (perry-runtime/src/ui_text_registry.rs: js_state_init / js_state_get /
// js_state_set + perry_arkts_set_text).
const keyedStates = new Map();    // synth id -> current value
const uiTextIds = new Map();      // text id  -> [DOM element handle, ...]
const keyedNavstack = new Map();  // synth id -> [{ name, widget }, ...]
const keyedForeach = new Map();   // synth id -> [{ host, render }, ...]

function uiTextRegister(handle, id) {
    let list = uiTextIds.get(id);
    if (!list) { list = []; uiTextIds.set(id, list); }
    list.push(handle);
}

function perry_ui_set_text(id, value) {
    const list = uiTextIds.get(id);
    if (!list) return;
    const str = (value === undefined || value === null) ? "" : String(value);
    for (const h of list) { const el = getHandle(h); if (el) el.textContent = str; }
}

function perry_ui_state_init(id, initial) { keyedStates.set(id, initial); }
function perry_ui_state_get_keyed(id) {
    return keyedStates.has(id) ? keyedStates.get(id) : undefined;
}
function perry_ui_state_set_keyed(id, value) {
    keyedStates.set(id, value);
    perry_ui_set_text(id, value);          // reactive Text re-render
    navstackDispatchKeyed(id, value);      // route visibility
    foreachDispatchKeyed(id, value);       // dynamic-list re-render
}

function foreachRenderKeyed(host, render, value) {
    const p = getHandle(host); if (!p) return;
    while (p.firstChild) p.removeChild(p.firstChild);
    const count = typeof value === "number" ? value : (Array.isArray(value) ? value.length : 0);
    for (let i = 0; i < count; i++) {
        const child = (typeof render === "function") ? render(i) : undefined;
        const childEl = getHandle(child);
        if (childEl) p.appendChild(childEl);
    }
}
function foreachDispatchKeyed(id, value) {
    const binds = keyedForeach.get(id); if (!binds) return;
    for (const b of binds) foreachRenderKeyed(b.host, b.render, value);
}
function perry_ui_foreach_register(id, host, render) {
    let list = keyedForeach.get(id);
    if (!list) { list = []; keyedForeach.set(id, list); }
    list.push({ host, render });
    if (keyedStates.has(id)) foreachRenderKeyed(host, render, keyedStates.get(id));
}

function navstackDispatchKeyed(id, value) {
    const routes = keyedNavstack.get(id); if (!routes) return;
    const cur = (value === undefined || value === null) ? "" : String(value);
    for (const r of routes) { const el = getHandle(r.widget); if (el) el.style.display = (r.name === cur) ? "" : "none"; }
}
function perry_ui_navstack_register_route(id, name, body) {
    let list = keyedNavstack.get(id);
    if (!list) { list = []; keyedNavstack.set(id, list); }
    list.push({ name, widget: body });
    if (keyedStates.has(id)) {
        const cur = String(keyedStates.get(id));
        const el = getHandle(body); if (el && name !== cur) el.style.display = "none";
    }
}

// --- System APIs ---
function perry_system_open_url(url) {
    window.open(url, "_blank");
}

function perry_system_is_dark_mode() {
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches ? 1.0 : 0.0;
}

function perry_system_preferences_get(key) {
    return localStorage.getItem(key) || "";
}

function perry_system_preferences_set(key, value) {
    localStorage.setItem(key, value);
}

// perry/system hapticPlay — Vibration API. Pattern lengths approximate
// the native semantics (error = double buzz). Silently a no-op in
// browsers without navigator.vibrate (Safari, all iOS browsers) and in
// non-window contexts, matching the "no haptic engine" API contract.
function perry_system_haptic_play(type) {
    if (typeof navigator === "undefined" || typeof navigator.vibrate !== "function") return;
    const patterns = {
        success: 50,
        error: [50, 80, 50],
        warning: [30, 50, 30],
        light: 10,
        medium: 20,
        heavy: 40,
        click: 10,
        selection: 10,
        directionUp: 15,
        directionDown: 15,
        start: 25,
        stop: 25,
    };
    try { navigator.vibrate(patterns[type] ?? 10); } catch (e) { /* no-op */ }
}

// --- Audio Capture (Web Audio API) ---
let _perry_audio_ctx = null;
let _perry_audio_analyser = null;
let _perry_audio_stream = null;
let _perry_audio_db = 0;
let _perry_audio_peak = 0;
let _perry_audio_waveform = [];
const _PERRY_WAVEFORM_SIZE = 256;

// A-weighting approximation for Web (simplified single-pole filter)
// Full IIR is CPU-intensive in JS; this gives a reasonable approximation.
function _perry_compute_db(analyser) {
    const bufLen = analyser.fftSize;
    const buf = new Float32Array(bufLen);
    analyser.getFloatTimeDomainData(buf);

    let sumSq = 0;
    let peak = 0;
    for (let i = 0; i < bufLen; i++) {
        const s = buf[i];
        const abs = Math.abs(s);
        if (abs > peak) peak = abs;
        sumSq += s * s;
    }
    const rms = Math.sqrt(sumSq / bufLen);
    const dbRaw = rms > 1e-10 ? 20 * Math.log10(rms) + 110 : 0;
    const dbClamped = Math.max(0, Math.min(140, dbRaw));

    // EMA smoothing
    const alpha = 0.3;
    _perry_audio_db += alpha * (dbClamped - _perry_audio_db);
    _perry_audio_peak = peak;

    // Waveform ring buffer
    _perry_audio_waveform.push(_perry_audio_db);
    if (_perry_audio_waveform.length > _PERRY_WAVEFORM_SIZE) {
        _perry_audio_waveform.shift();
    }
}

function perry_system_audio_start() {
    if (_perry_audio_ctx) return 1.0;
    try {
        const ctx = new (window.AudioContext || window.webkitAudioContext)();
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 2048;

        navigator.mediaDevices.getUserMedia({ audio: true }).then(stream => {
            _perry_audio_stream = stream;
            const source = ctx.createMediaStreamSource(stream);
            source.connect(analyser);
            _perry_audio_ctx = ctx;
            _perry_audio_analyser = analyser;

            // Poll at ~60fps
            const poll = () => {
                if (!_perry_audio_ctx) return;
                _perry_compute_db(_perry_audio_analyser);
                requestAnimationFrame(poll);
            };
            requestAnimationFrame(poll);
        }).catch(() => {
            _perry_audio_ctx = null;
        });
        return 1.0;
    } catch (e) {
        return 0.0;
    }
}

function perry_system_audio_stop() {
    if (_perry_audio_stream) {
        _perry_audio_stream.getTracks().forEach(t => t.stop());
        _perry_audio_stream = null;
    }
    if (_perry_audio_ctx) {
        _perry_audio_ctx.close();
        _perry_audio_ctx = null;
        _perry_audio_analyser = null;
    }
}

function perry_system_audio_get_level() {
    return _perry_audio_db;
}

function perry_system_audio_get_peak() {
    return _perry_audio_peak;
}

function perry_system_audio_get_waveform(count) {
    // Return a JS array (Perry web runtime handles arrays natively)
    const n = Math.min(Math.floor(count), _perry_audio_waveform.length);
    return _perry_audio_waveform.slice(-n);
}

function perry_system_get_device_model() {
    return navigator.userAgent || "Web";
}

// Web audio recording to WAV file (stub for now, but structure added)
let _perry_audio_recording = false;
let _perry_audio_output_filename = null;
let _perry_audio_media_recorder = null;
let _perry_audio_recorded_chunks = [];

function perry_system_audio_set_output_filename(filename) {
    _perry_audio_output_filename = filename;
}

function perry_system_audio_start_recording() {
    if (_perry_audio_recording) return;
    if (!_perry_audio_stream) return;

    try {
        _perry_audio_recorded_chunks = [];
        _perry_audio_media_recorder = new MediaRecorder(_perry_audio_stream);
        _perry_audio_media_recorder.ondataavailable = (event) => {
            if (event.data.size > 0) {
                _perry_audio_recorded_chunks.push(event.data);
            }
        };
        _perry_audio_media_recorder.onstop = () => {
            const blob = new Blob(_perry_audio_recorded_chunks, { type: 'audio/wav' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = _perry_audio_output_filename || 'recording.wav';
            a.click();
            URL.revokeObjectURL(url);
        };
        _perry_audio_media_recorder.start();
        _perry_audio_recording = true;
    } catch (e) {
        console.error("Failed to start recording:", e);
    }
}

function perry_system_audio_stop_recording() {
    if (!_perry_audio_recording) return;
    if (_perry_audio_media_recorder) {
        _perry_audio_media_recorder.stop();
        _perry_audio_media_recorder = null;
    }
    _perry_audio_recording = false;
}

// --- Canvas Operations ---
function perry_ui_canvas_fill_rect(h, x, y, w, ht) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.fillRect(x, y, w, ht);
}

function perry_ui_canvas_stroke_rect(h, x, y, w, ht) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.strokeRect(x, y, w, ht);
}

function perry_ui_canvas_clear_rect(h, x, y, w, ht) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.clearRect(x, y, w, ht);
}

function perry_ui_canvas_set_fill_color(h, r, g, b, a) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.fillStyle = `rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

function perry_ui_canvas_set_stroke_color(h, r, g, b, a) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.strokeStyle = `rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

function perry_ui_canvas_begin_path(h) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.beginPath();
}

function perry_ui_canvas_move_to(h, x, y) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.moveTo(x, y);
}

function perry_ui_canvas_line_to(h, x, y) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.lineTo(x, y);
}

function perry_ui_canvas_arc(h, x, y, radius, startAngle, endAngle) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.arc(x, y, radius, startAngle, endAngle);
}

function perry_ui_canvas_close_path(h) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.closePath();
}

function perry_ui_canvas_fill(h) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.fill();
}

function perry_ui_canvas_stroke(h) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.stroke();
}

function perry_ui_canvas_set_line_width(h, w) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.lineWidth = w;
}

function perry_ui_canvas_fill_text(h, text, x, y) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.fillText(text, x, y);
}

function perry_ui_canvas_set_font(h, font) {
    const el = getHandle(h);
    if (el && el._ctx) el._ctx.font = font;
}

// --- App Lifecycle ---
function perry_ui_app_set_body(app_h, root_h) {
    const root = getHandle(app_h);
    const child = getHandle(root_h);
    if (root && child) { root.innerHTML = ""; root.appendChild(child); }
}

function perry_ui_app_set_min_size(app_h, w, h) {
    const root = getHandle(app_h);
    if (root) { root.style.minWidth = w + "px"; root.style.minHeight = h + "px"; }
}

function perry_ui_app_set_max_size(app_h, w, h) {
    const root = getHandle(app_h);
    if (root) { root.style.maxWidth = w + "px"; root.style.maxHeight = h + "px"; }
}

function perry_ui_app_on_activate(callback) {
    if (typeof callback === "function") {
        document.addEventListener("visibilitychange", () => { if (!document.hidden) callback(); });
    }
}

function perry_ui_app_on_terminate(callback) {
    if (typeof callback === "function") {
        window.addEventListener("beforeunload", () => callback());
    }
}

function perry_ui_app_set_timer(interval_ms, callback) {
    if (typeof callback === "function") setInterval(callback, interval_ms);
}

// --- Multi-Window ---
const _windows = new Map();
let _nextWindowId = 1;

function perry_ui_window_create(title, width, height) {
    const overlay = document.createElement("div");
    overlay.style.cssText = "position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.3);display:none;z-index:1000;justify-content:center;align-items:center;";
    const win = document.createElement("div");
    win.style.cssText = `background:#fff;border-radius:8px;box-shadow:0 4px 24px rgba(0,0,0,0.2);width:${width}px;min-height:${height}px;padding:16px;position:relative;`;
    if (title) { const t = document.createElement("div"); t.textContent = title; t.style.fontWeight = "bold"; t.style.marginBottom = "8px"; win.appendChild(t); }
    overlay.appendChild(win);
    document.body.appendChild(overlay);
    const id = _nextWindowId++;
    _windows.set(id, { overlay, win, body: null });
    return id;
}

function perry_ui_window_set_body(window_h, widget_h) {
    const w = _windows.get(window_h);
    const child = getHandle(widget_h);
    if (w && child) { w.body = child; w.win.appendChild(child); }
}

function perry_ui_window_show(window_h) {
    const w = _windows.get(window_h);
    if (w) w.overlay.style.display = "flex";
}

function perry_ui_window_close(window_h) {
    const w = _windows.get(window_h);
    if (w) w.overlay.style.display = "none";
}

// --- State (canonical function names) ---
function perry_ui_state_create(initial) { return stateCreate(initial); }
function perry_ui_state_get(h) { return stateGet(h); }
function perry_ui_state_set(h, v) { stateSet(h, v); }

function perry_ui_state_bind_textfield(stateH, widgetH) {
    const el = getHandle(widgetH);
    if (!el) return;
    stateSubscribe(stateH, (v) => { el.value = String(v); });
    el.value = String(stateGet(stateH) || "");
    el.addEventListener("input", () => stateSet(stateH, el.value));
}

// --- Widget Operations ---
function perry_ui_widget_add_child_at(parent_h, child_h, index) {
    const parent = getHandle(parent_h);
    const child = getHandle(child_h);
    if (parent && child) {
        const ref = parent.children[Math.floor(index)] || null;
        parent.insertBefore(child, ref);
    }
}

function perry_ui_set_widget_hidden(h, hidden) {
    const el = getHandle(h);
    if (el) el.style.display = hidden ? "none" : "";
}

function perry_ui_lazyvstack_create(count, renderFn) {
    const scroll = document.createElement("div");
    scroll.style.overflow = "auto"; scroll.style.flex = "1";
    const inner = document.createElement("div");
    inner.style.display = "flex"; inner.style.flexDirection = "column";
    scroll.appendChild(inner);
    scroll._inner = inner; scroll._renderFn = renderFn;
    if (typeof renderFn === "function") {
        for (let i = 0; i < count; i++) renderFn(i);
    }
    return wrapWidget(allocHandle(scroll));
}

function perry_ui_lazyvstack_update(h, count) {
    const el = getHandle(h);
    if (el && el._inner && el._renderFn) {
        el._inner.innerHTML = "";
        for (let i = 0; i < count; i++) el._renderFn(i);
    }
}

// --- Table (DOM <table> implementation) ---
function perry_ui_table_create(rowCount, colCount, renderFn) {
    const scroll = document.createElement("div");
    scroll.style.overflow = "auto"; scroll.style.flex = "1";
    const tbl = document.createElement("table");
    tbl.style.borderCollapse = "collapse"; tbl.style.width = "100%";
    const thead = document.createElement("thead");
    const headerRow = document.createElement("tr");
    for (let c = 0; c < colCount; c++) {
        const th = document.createElement("th");
        th.style.borderBottom = "1px solid #ccc"; th.style.padding = "4px 8px";
        headerRow.appendChild(th);
    }
    thead.appendChild(headerRow);
    const tbody = document.createElement("tbody");
    tbl.appendChild(thead); tbl.appendChild(tbody);
    scroll.appendChild(tbl);
    scroll._tbl = tbl; scroll._thead = thead; scroll._tbody = tbody;
    scroll._colCount = colCount; scroll._renderFn = renderFn;
    scroll._selectedRow = -1; scroll._onRowSelect = null;
    function buildRows(rc) {
        tbody.innerHTML = "";
        for (let r = 0; r < rc; r++) {
            const tr = document.createElement("tr");
            (function(row) {
                tr.onclick = function() {
                    scroll._selectedRow = row;
                    if (typeof scroll._onRowSelect === "function") scroll._onRowSelect(row);
                };
            })(r);
            for (let c = 0; c < colCount; c++) {
                const td = document.createElement("td");
                td.style.padding = "4px 8px"; td.style.borderBottom = "1px solid #eee";
                if (typeof renderFn === "function") renderFn(r, c);
                tr.appendChild(td);
            }
            tbody.appendChild(tr);
        }
    }
    buildRows(rowCount);
    scroll._buildRows = buildRows;
    return wrapWidget(allocHandle(scroll));
}
function perry_ui_table_set_column_header(h, col, title) {
    const el = getHandle(h);
    if (el && el._thead) {
        const ths = el._thead.querySelectorAll("th");
        if (ths[col]) ths[col].textContent = title || "";
    }
}
function perry_ui_table_set_column_width(h, col, width) {
    const el = getHandle(h);
    if (el && el._thead) {
        const ths = el._thead.querySelectorAll("th");
        if (ths[col]) ths[col].style.width = width + "px";
    }
}
function perry_ui_table_update_row_count(h, count) {
    const el = getHandle(h);
    if (el && el._buildRows) el._buildRows(count);
}
function perry_ui_table_set_on_row_select(h, cb) {
    const el = getHandle(h);
    if (el) el._onRowSelect = cb;
}
function perry_ui_table_get_selected_row(h) {
    const el = getHandle(h);
    return el ? el._selectedRow : -1;
}

// --- Text Operations ---
function perry_ui_text_set_string(h, text) {
    const el = getHandle(h);
    if (el) el.textContent = text;
}

function perry_ui_text_set_selectable(h, selectable) {
    const el = getHandle(h);
    if (el) el.style.userSelect = selectable ? "text" : "none";
}

// --- Button Operations ---
function perry_ui_button_set_bordered(h, bordered) {
    const el = getHandle(h);
    if (el) el.style.border = bordered ? "1px solid #ccc" : "none";
}

function perry_ui_button_set_title(h, title) {
    const el = getHandle(h);
    if (el) el.textContent = title;
}

function perry_ui_button_set_text_color(h, r, g, b, a) {
    const el = getHandle(h);
    if (el) el.style.color = `rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

var _sfSymbolSVGs = {
    // Activity bar / sidebar
    "folder":           '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M2 4a2 2 0 012-2h4l2 2h6a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V4z"/></svg>',
    "folder.fill":      '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M2 4a2 2 0 012-2h4l2 2h6a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V4z"/></svg>',
    "magnifyingglass":  '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M8 4a4 4 0 100 8 4 4 0 000-8zM2 8a6 6 0 1110.89 3.476l4.817 4.817a1 1 0 01-1.414 1.414l-4.816-4.816A6 6 0 012 8z" clip-rule="evenodd"/></svg>',
    "sparkles":         '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M10 2l1.5 4.5L16 8l-4.5 1.5L10 14l-1.5-4.5L4 8l4.5-1.5L10 2zM15 12l.75 2.25L18 15l-2.25.75L15 18l-.75-2.25L12 15l2.25-.75L15 12z"/></svg>',
    "gearshape":        '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M11.49 3.17c-.38-1.56-2.6-1.56-2.98 0a1.532 1.532 0 01-2.286.948c-1.372-.836-2.942.734-2.106 2.106.54.886.061 2.042-.947 2.287-1.561.379-1.561 2.6 0 2.978a1.532 1.532 0 01.947 2.287c-.836 1.372.734 2.942 2.106 2.106a1.532 1.532 0 012.287.947c.379 1.561 2.6 1.561 2.978 0a1.533 1.533 0 012.287-.947c1.372.836 2.942-.734 2.106-2.106a1.533 1.533 0 01.947-2.287c1.561-.379 1.561-2.6 0-2.978a1.532 1.532 0 01-.947-2.287c.836-1.372-.734-2.942-2.106-2.106a1.532 1.532 0 01-2.287-.947zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd"/></svg>',
    "arrow.triangle.2.circlepath": '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M4 2a1 1 0 011 1v2.101a7.002 7.002 0 0111.601 2.566 1 1 0 11-1.885.666A5.002 5.002 0 005.999 7H9a1 1 0 010 2H4a1 1 0 01-1-1V3a1 1 0 011-1zm.008 9.057a1 1 0 011.276.61A5.002 5.002 0 0014.001 13H11a1 1 0 110-2h5a1 1 0 011 1v5a1 1 0 11-2 0v-2.101a7.002 7.002 0 01-11.601-2.566 1 1 0 01.61-1.276z" clip-rule="evenodd"/></svg>',
    "arrow.triangle.branch": '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M5 3a2 2 0 00-2 2v1a2 2 0 002 2h1v3a2 2 0 002 2h2v1a2 2 0 002 2h1a2 2 0 002-2v-1a2 2 0 00-2-2h-1V8a2 2 0 00-2-2H8V5a2 2 0 00-2-2H5z" clip-rule="evenodd"/></svg>',
    // Editor / explorer
    "xmark":            '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd"/></svg>',
    "chevron.right":    '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M7.293 14.707a1 1 0 010-1.414L10.586 10 7.293 6.707a1 1 0 011.414-1.414l4 4a1 1 0 010 1.414l-4 4a1 1 0 01-1.414 0z" clip-rule="evenodd"/></svg>',
    "chevron.down":     '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M5.293 7.293a1 1 0 011.414 0L10 10.586l3.293-3.293a1 1 0 111.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 010-1.414z" clip-rule="evenodd"/></svg>',
    "ellipsis":         '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M6 10a2 2 0 11-4 0 2 2 0 014 0zm6 0a2 2 0 11-4 0 2 2 0 014 0zm6 0a2 2 0 11-4 0 2 2 0 014 0z"/></svg>',
    "doc.badge.plus":   '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M6 2a2 2 0 00-2 2v12a2 2 0 002 2h8a2 2 0 002-2V7.414A2 2 0 0015.414 6L12 2.586A2 2 0 0010.586 2H6zm5 6a1 1 0 10-2 0v2H7a1 1 0 100 2h2v2a1 1 0 102 0v-2h2a1 1 0 100-2h-2V8z" clip-rule="evenodd"/></svg>',
    "folder.badge.plus":'<svg viewBox="0 0 20 20" fill="currentColor"><path d="M2 4a2 2 0 012-2h4l2 2h6a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V4zm9 4a1 1 0 10-2 0v1H8a1 1 0 100 2h1v1a1 1 0 102 0v-1h1a1 1 0 100-2h-1V8z"/></svg>',
    "arrow.down.right.and.arrow.up.left": '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M15 3a1 1 0 00-1 1v4.586l-4.293-4.293a1 1 0 00-1.414 1.414L12.586 10H8a1 1 0 100 2h5.586l-4.293 4.293a1 1 0 001.414 1.414L15 13.414V18a1 1 0 102 0V4a1 1 0 00-1-1zM5 17a1 1 0 001-1v-4.586l4.293 4.293a1 1 0 001.414-1.414L7.414 10H12a1 1 0 100-2H6.414l4.293-4.293a1 1 0 00-1.414-1.414L5 6.586V2a1 1 0 10-2 0v14a1 1 0 001 1z" clip-rule="evenodd"/></svg>',
    // Debug
    "play.fill":        '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM9.555 7.168A1 1 0 008 8v4a1 1 0 001.555.832l3-2a1 1 0 000-1.664l-3-2z" clip-rule="evenodd"/></svg>',
    "pause.fill":       '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zM7 8a1 1 0 012 0v4a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v4a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd"/></svg>',
    "stop.fill":        '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8 7a1 1 0 00-1 1v4a1 1 0 001 1h4a1 1 0 001-1V8a1 1 0 00-1-1H8z" clip-rule="evenodd"/></svg>',
    "arrow.right":      '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10.293 3.293a1 1 0 011.414 0l6 6a1 1 0 010 1.414l-6 6a1 1 0 01-1.414-1.414L14.586 11H3a1 1 0 110-2h11.586l-4.293-4.293a1 1 0 010-1.414z" clip-rule="evenodd"/></svg>',
    "arrow.down.right": '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M14 13.5a.5.5 0 01-.5.5h-7a.5.5 0 010-1h5.793L5.146 5.854a.5.5 0 01.708-.708L13 12.293V6.5a.5.5 0 011 0v7z" clip-rule="evenodd"/></svg>',
    "arrow.up.left":    '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M6 6.5a.5.5 0 01.5-.5h7a.5.5 0 010 1H7.707l7.147 7.146a.5.5 0 01-.708.708L7 7.707V13.5a.5.5 0 01-1 0v-7z" clip-rule="evenodd"/></svg>',
    // Terminal
    "arrow.up.left.and.arrow.down.right": '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M3 4a1 1 0 011-1h4a1 1 0 010 2H5.414l4.293 4.293a1 1 0 01-1.414 1.414L4 6.414V9a1 1 0 11-2 0V4a1 1 0 011-1zm10 12a1 1 0 01-1 1h-1a1 1 0 010-2h2.586l-4.293-4.293a1 1 0 011.414-1.414L15 13.586V11a1 1 0 112 0v5a1 1 0 01-1 1h-3z" clip-rule="evenodd"/></svg>',
    // File type icons
    "doc.text":         '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 6a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd"/></svg>',
    "doc.on.doc":       '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M7 3a1 1 0 000 2h6a1 1 0 100-2H7zM4 7a1 1 0 011-1h10a1 1 0 110 2H5a1 1 0 01-1-1zm-2 4a2 2 0 012-2h12a2 2 0 012 2v4a2 2 0 01-2 2H4a2 2 0 01-2-2v-4z"/></svg>',
    "circle.fill":      '<svg viewBox="0 0 20 20" fill="currentColor"><circle cx="10" cy="10" r="6"/></svg>',
    "plus":             '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" clip-rule="evenodd"/></svg>',
    "trash":            '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd"/></svg>',
    "square.and.arrow.up": '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M3 17a1 1 0 011-1h12a1 1 0 110 2H4a1 1 0 01-1-1zm3.293-7.707a1 1 0 011.414 0L9 10.586V3a1 1 0 112 0v7.586l1.293-1.293a1 1 0 111.414 1.414l-3 3a1 1 0 01-1.414 0l-3-3a1 1 0 010-1.414z"/></svg>',
    "ladybug":          '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 2a1 1 0 011 1v1.323l3.954 1.582 1.599-.8a1 1 0 01.894 1.79l-1.233.616c.292.615.464 1.295.464 2.014v.75h2.322a1 1 0 110 2H14.68a5.027 5.027 0 01-.705 1.563l1.732 1a1 1 0 01-1 1.732l-1.732-1A4.988 4.988 0 0110 17a4.988 4.988 0 01-2.975-.98l-1.732 1a1 1 0 01-1-1.732l1.732-1A5.027 5.027 0 015.32 12.725H3a1 1 0 110-2h2.322v-.75c0-.72.172-1.399.464-2.014l-1.233-.616a1 1 0 11.894-1.79l1.599.8L11 4.323V3a1 1 0 011-1z" clip-rule="evenodd"/></svg>',
    "puzzlepiece.extension": '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M10 3.5a1.5 1.5 0 013 0V4a1 1 0 001 1h1.5a2 2 0 012 2v1.5a1 1 0 01-1 1H16a1.5 1.5 0 000 3h.5a1 1 0 011 1V15a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h1.5a1 1 0 001-1v-.5z"/></svg>',
    "terminal":         '<svg viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M2 5a2 2 0 012-2h12a2 2 0 012 2v10a2 2 0 01-2 2H4a2 2 0 01-2-2V5zm3.293 1.293a1 1 0 011.414 0l3 3a1 1 0 010 1.414l-3 3a1 1 0 01-1.414-1.414L7.586 10 5.293 7.707a1 1 0 010-1.414zM11 12a1 1 0 100 2h3a1 1 0 100-2h-3z" clip-rule="evenodd"/></svg>',
    // Tab file icons (swift, etc.)
    "swift":            '<svg viewBox="0 0 20 20" fill="currentColor"><path d="M13.7 3.3C12.5 2.7 11.2 2.4 10 2.4c-4.2 0-7.6 3.4-7.6 7.6 0 2 .8 3.8 2 5.2C5 14.4 7 12.6 8.5 11c-1.4-.8-2.4-2-3-3.2 1.2 1 2.5 1.7 3.7 2 1.6-1.6 2.8-3.4 3.5-5-1 1.2-2.3 2.5-3.7 3.5 1 .3 2 .3 2.8.1.8-.3 1.5-.8 2-1.5.3-.5.5-1.1.4-1.7-.1-.7-.3-1.3-.5-1.9z"/></svg>',
};

function perry_ui_button_set_image(h, name) {
    var el = getHandle(h);
    if (!el) return;
    var svg = _sfSymbolSVGs[name];
    if (svg) {
        // Create inline SVG icon
        var iconSpan = document.createElement("span");
        iconSpan.className = "perry-icon";
        iconSpan.innerHTML = svg;
        iconSpan.style.display = "inline-flex";
        iconSpan.style.width = "16px";
        iconSpan.style.height = "16px";
        iconSpan.style.verticalAlign = "middle";
        iconSpan.style.flexShrink = "0";
        var svgEl = iconSpan.querySelector("svg");
        if (svgEl) {
            svgEl.style.width = "100%";
            svgEl.style.height = "100%";
        }
        // Store existing text
        var existingText = el.textContent || "";
        el.textContent = "";
        el.style.display = "inline-flex";
        el.style.alignItems = "center";
        el.style.gap = "4px";
        el.appendChild(iconSpan);
        if (existingText.trim()) {
            el.appendChild(document.createTextNode(existingText));
        }
        // Store icon element for tint color changes
        el._perryIcon = iconSpan;
    } else {
        // Fallback: show name as text (shouldn't happen if map is complete)
        el.textContent = name;
    }
}

function perry_ui_button_set_content_tint_color(h, r, g, b, a) {
    const el = getHandle(h);
    if (el) el.style.color = `rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

function perry_ui_widget_set_width(h, w) {
    const el = getHandle(h);
    if (el) { el.style.width = w + "px"; el.style.minWidth = w + "px"; el.style.maxWidth = w + "px"; el.style.flexShrink = "0"; }
}

function perry_ui_widget_set_hugging(h, priority) {
    const el = getHandle(h);
    if (!el) return;
    if (priority <= 250) {
        el.style.flex = "1 1 0%";
    } else {
        el.style.flex = "0 0 auto";
    }
}

function perry_ui_open_folder_dialog(callback) {
    if (window.showDirectoryPicker) {
        window.showDirectoryPicker().then(function(dirHandle) {
            window.__perryDirHandle = dirHandle;
            window.__perryFileCache = {};
            _buildFileCacheAsync(dirHandle, "/" + dirHandle.name).then(function() {
                return _prefetchAllFiles();
            }).then(function() {
                if (typeof callback === "function") callback("/" + dirHandle.name);
            });
        }).catch(function() { /* user cancelled */ });
    } else {
        // Fallback: input with webkitdirectory
        const input = document.createElement("input");
        input.type = "file";
        input.webkitdirectory = true;
        input.addEventListener("change", function() {
            if (input.files.length > 0 && typeof callback === "function") {
                const rootName = input.files[0].webkitRelativePath.split("/")[0];
                window.__perryFileCache = {};
                _buildFileCacheFromFileList(input.files, rootName);
                _prefetchAllFilesFromFileList(input.files).then(function() {
                    callback("/" + rootName);
                });
            }
        });
        input.click();
    }
}

// Eagerly cache directory structure from File System Access API
async function _buildFileCacheAsync(dirHandle, path) {
    if (!window.__perryFileCache) window.__perryFileCache = {};
    const children = [];
    for await (const [name, handle] of dirHandle.entries()) {
        const childPath = path + "/" + name;
        const isDir = handle.kind === "directory";
        children.push(name);
        window.__perryFileCache[childPath] = { isDir: isDir, handle: handle, content: null, children: isDir ? [] : null };
        if (isDir) {
            await _buildFileCacheAsync(handle, childPath);
        }
    }
    if (window.__perryFileCache[path]) {
        window.__perryFileCache[path].children = children;
    } else {
        window.__perryFileCache[path] = { isDir: true, handle: dirHandle, content: null, children: children };
    }
}

// Fallback: build cache from FileList (webkitdirectory)
function _buildFileCacheFromFileList(files, rootName) {
    const dirs = {};
    for (let i = 0; i < files.length; i++) {
        const rel = "/" + files[i].webkitRelativePath;
        const parts = rel.split("/");
        // Register all parent directories
        for (let j = 2; j < parts.length; j++) {
            const dirPath = parts.slice(0, j).join("/");
            if (!dirs[dirPath]) {
                dirs[dirPath] = [];
            }
            const childName = parts[j];
            if (dirs[dirPath].indexOf(childName) < 0) dirs[dirPath].push(childName);
        }
        // Register the file
        window.__perryFileCache[rel] = { isDir: false, handle: files[i], content: null, children: null };
    }
    // Register directories
    for (const dirPath in dirs) {
        if (!window.__perryFileCache[dirPath]) {
            window.__perryFileCache[dirPath] = { isDir: true, handle: null, content: null, children: dirs[dirPath] };
        }
    }
}

// Read all files from a FileList (webkitdirectory fallback)
async function _prefetchAllFilesFromFileList(files) {
    for (var i = 0; i < files.length; i++) {
        var file = files[i];
        var path = "/" + file.webkitRelativePath;
        try {
            var text = await file.text();
            if (window.__perryFileCache[path]) {
                window.__perryFileCache[path].content = text;
            }
        } catch(e) {}
    }
}

function perry_ui_embed_ns_view(handleId) {
    // On web, handleId is an editor handle ID. Look up the DOM element and wrap it.
    const el = getHandle(handleId);
    if (!el) return wrapWidget(allocHandle(document.createElement("div")));
    const container = document.createElement("div");
    container.style.flex = "1 1 0%";
    container.style.display = "flex";
    container.style.overflow = "hidden";
    container.appendChild(el);
    return wrapWidget(allocHandle(container));
}

// --- TextField Operations ---
function perry_ui_textfield_focus(h) {
    const el = getHandle(h);
    if (el) el.focus();
}

function perry_ui_textfield_set_string(h, text) {
    const el = getHandle(h);
    if (el) el.value = text;
}

// --- ScrollView Operations ---
function perry_ui_scrollview_set_child(scroll_h, child_h) {
    const scroll = getHandle(scroll_h);
    const child = getHandle(child_h);
    if (scroll && child) { scroll.innerHTML = ""; scroll.appendChild(child); }
}

function perry_ui_scrollview_scroll_to(scroll_h, child_h) {
    const scroll = getHandle(scroll_h);
    const child = getHandle(child_h);
    if (scroll && child) child.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

function perry_ui_scrollview_get_offset(scroll_h) {
    const el = getHandle(scroll_h);
    return el ? el.scrollTop : 0;
}

function perry_ui_scrollview_set_offset(scroll_h, offset) {
    const el = getHandle(scroll_h);
    if (el) el.scrollTop = offset;
}

// --- Styling ---
function perry_ui_widget_set_background_gradient(h, r1, g1, b1, a1, r2, g2, b2, a2, direction) {
    const el = getHandle(h);
    if (!el) return;
    const c1 = `rgba(${Math.round(r1*255)},${Math.round(g1*255)},${Math.round(b1*255)},${a1})`;
    const c2 = `rgba(${Math.round(r2*255)},${Math.round(g2*255)},${Math.round(b2*255)},${a2})`;
    const dir = direction < 0.5 ? "to bottom" : "to right";
    el.style.background = `linear-gradient(${dir}, ${c1}, ${c2})`;
}


function perry_ui_load_image(url) {
    const key = String(url || "");
    if (perryImageCache.has(key)) return perryImageCache.get(key).promise;
    const img = new Image();
    const asset = { url: key, element: img, width: 0, height: 0, ready: false, error: null };
    const promise = new Promise((resolve, reject) => {
        img.onload = () => {
            asset.width = img.naturalWidth || img.width || 0;
            asset.height = img.naturalHeight || img.height || 0;
            asset.ready = true;
            resolve(asset);
        };
        img.onerror = () => {
            asset.error = new Error(`Failed to load image: ${key}`);
            reject(asset.error);
        };
        img.src = key;
        if (img.decode) img.decode().then(() => {
            if (!asset.ready) {
                asset.width = img.naturalWidth || img.width || 0;
                asset.height = img.naturalHeight || img.height || 0;
                asset.ready = true;
                resolve(asset);
            }
        }).catch(() => { /* onerror covers network/decode failures */ });
    });
    asset.promise = promise;
    perryImageCache.set(key, asset);
    return promise;
}

function perry_ui_canvas_draw_image(h, image, ...args) {
    const el = (typeof getHandle === "function") ? getHandle(h) : uiGet(h);
    const ctx = el && el._ctx;
    const asset = image && image.element ? image : null;
    if (!ctx || !asset || !asset.ready || !asset.element) return;
    if (args.length === 2) {
        ctx.drawImage(asset.element, args[0], args[1]);
    } else if (args.length === 4) {
        ctx.drawImage(asset.element, args[0], args[1], args[2], args[3]);
    } else if (args.length >= 8) {
        const [sx, sy, sw, sh, dx, dy, dw, dh] = args;
        const srcW = sw > 0 ? sw : asset.width;
        const srcH = sh > 0 ? sh : asset.height;
        const dstW = dw > 0 ? dw : srcW;
        const dstH = dh > 0 ? dh : srcH;
        if (srcW <= 0 || srcH <= 0 || dstW <= 0 || dstH <= 0) return;
        ctx.drawImage(asset.element, sx, sy, srcW, srcH, dx, dy, dstW, dstH);
    }
}

function perry_ui_canvas_fill_gradient(h, r1, g1, b1, a1, r2, g2, b2, a2, direction) {
    const el = getHandle(h);
    if (!el || !el._ctx) return;
    const ctx = el._ctx;
    const grad = direction < 0.5
        ? ctx.createLinearGradient(0, 0, 0, el.height)
        : ctx.createLinearGradient(0, 0, el.width, 0);
    grad.addColorStop(0, `rgba(${Math.round(r1*255)},${Math.round(g1*255)},${Math.round(b1*255)},${a1})`);
    grad.addColorStop(1, `rgba(${Math.round(r2*255)},${Math.round(g2*255)},${Math.round(b2*255)},${a2})`);
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, el.width, el.height);
}

// --- Layout with Insets ---
function perry_ui_vstack_create_with_insets(spacing, top, left, bottom, right) {
    const el = document.createElement("div");
    el.style.display = "flex"; el.style.flexDirection = "column"; el.style.gap = spacing + "px";
    el.style.padding = `${top}px ${right}px ${bottom}px ${left}px`;
    return wrapWidget(allocHandle(el));
}

function perry_ui_hstack_create_with_insets(spacing, top, left, bottom, right) {
    const el = document.createElement("div");
    el.style.display = "flex"; el.style.flexDirection = "row"; el.style.gap = spacing + "px";
    el.style.alignItems = "center";
    el.style.padding = `${top}px ${right}px ${bottom}px ${left}px`;
    return wrapWidget(allocHandle(el));
}

// --- Navigation ---
function perry_ui_navstack_push(h, body_h) {
    const nav = getHandle(h);
    const body = getHandle(body_h);
    if (!nav || !body) return;
    // Hide current children
    for (const child of nav.children) child.style.display = "none";
    nav.appendChild(body);
    if (!nav._navStack) nav._navStack = [];
    nav._navStack.push(body);
}

function perry_ui_navstack_pop(h) {
    const nav = getHandle(h);
    if (!nav || !nav._navStack || nav._navStack.length <= 1) return;
    const removed = nav._navStack.pop();
    if (removed) removed.style.display = "none";
    const top = nav._navStack[nav._navStack.length - 1];
    if (top) top.style.display = "";
}

// --- Picker Operations ---
function perry_ui_picker_add_item(h, title) {
    const el = getHandle(h);
    if (!el) return;
    const opt = document.createElement("option");
    opt.value = el.children.length;
    opt.textContent = title;
    el.appendChild(opt);
}

function perry_ui_picker_set_selected(h, index) {
    const el = getHandle(h);
    if (el) el.selectedIndex = index;
}

function perry_ui_picker_get_selected(h) {
    const el = getHandle(h);
    return el ? el.selectedIndex : -1;
}

// --- Image Operations ---
function perry_ui_image_create_symbol(name) {
    const el = document.createElement("span");
    el.textContent = name; // Use text as placeholder for symbols
    el.style.fontSize = "24px";
    return wrapWidget(allocHandle(el));
}

// #635: Image({ url, alt }) — single image by URL.
function perry_ui_image_create_url(url, alt) {
    const el = document.createElement("img");
    el.src = url || "";
    if (alt) el.alt = alt;
    el.style.objectFit = "cover";
    return wrapWidget(allocHandle(el));
}

function perry_ui_image_set_size(h, width, height) {
    const el = getHandle(h);
    if (el) { el.style.width = width + "px"; el.style.height = height + "px"; }
}

function perry_ui_image_set_tint(h, r, g, b, a) {
    const el = getHandle(h);
    if (el) el.style.color = `rgba(${Math.round(r*255)},${Math.round(g*255)},${Math.round(b*255)},${a})`;
}

// --- ProgressView ---
function perry_ui_progressview_set_value(h, value) {
    const el = getHandle(h);
    if (el) { el.removeAttribute("indeterminate"); el.value = value; }
}

// --- Menus ---
const _menus = new Map();
let _nextMenuId = 1;

function perry_ui_menu_create() {
    const id = _nextMenuId++;
    _menus.set(id, []);
    return id;
}

function perry_ui_menu_add_item(menu_h, title, callback, shortcut) {
    const items = _menus.get(menu_h);
    if (items) items.push({ type: "item", title, callback, shortcut: shortcut || undefined });
}

function perry_ui_menu_add_item_with_shortcut(menu_h, title, callback, shortcut) {
    const items = _menus.get(menu_h);
    if (items) items.push({ type: "item", title, callback, shortcut });
}

function perry_ui_menu_add_separator(menu_h) {
    const items = _menus.get(menu_h);
    if (items) items.push({ type: "separator" });
}

function perry_ui_menu_add_submenu(menu_h, title, submenu_h) {
    const items = _menus.get(menu_h);
    if (items) items.push({ type: "submenu", title, submenu: submenu_h });
}

const _menubars = new Map();
let _nextMenubarId = 1;

function perry_ui_menubar_create() {
    const id = _nextMenubarId++;
    _menubars.set(id, { menus: [] });
    return id;
}

function perry_ui_menubar_add_menu(bar_h, title, menu_h) {
    const bar = _menubars.get(bar_h);
    if (bar) bar.menus.push({ title, menu_h });
}

function perry_ui_menubar_attach(bar_h) {
    const bar = _menubars.get(bar_h);
    if (!bar) return;

    // Remove existing menubar if any
    const old = document.querySelector(".perry-menubar");
    if (old) old.remove();

    const barEl = document.createElement("div");
    barEl.className = "perry-menubar";
    barEl.style.cssText = "display:flex;background:#f0f0f0;border-bottom:1px solid #ccc;padding:0;font-family:system-ui,-apple-system,sans-serif;font-size:13px;user-select:none;position:relative;z-index:10000;";

    let openDropdown = null;
    let openTitle = null;

    function dismissAll() {
        if (openDropdown) { openDropdown.remove(); openDropdown = null; openTitle = null; }
    }

    function renderMenuItems(container, menu_h) {
        const items = _menus.get(menu_h);
        if (!items) return;
        for (const item of items) {
            if (item.type === "separator") {
                const sep = document.createElement("div");
                sep.style.cssText = "height:1px;background:#ccc;margin:4px 0;";
                container.appendChild(sep);
            } else if (item.type === "submenu") {
                const mi = document.createElement("div");
                mi.style.cssText = "padding:4px 24px 4px 16px;cursor:pointer;display:flex;justify-content:space-between;white-space:nowrap;position:relative;";
                mi.innerHTML = `<span>${item.title}</span><span style="margin-left:16px;color:#999;">▸</span>`;
                mi.addEventListener("mouseenter", () => {
                    mi.style.background = "#0066ff"; mi.style.color = "#fff";
                    // Show submenu
                    let sub = mi.querySelector(".perry-submenu");
                    if (!sub) {
                        sub = document.createElement("div");
                        sub.className = "perry-submenu";
                        sub.style.cssText = "position:absolute;left:100%;top:0;background:#fff;border:1px solid #ccc;border-radius:4px;box-shadow:0 2px 8px rgba(0,0,0,0.15);padding:4px 0;min-width:120px;color:#000;";
                        renderMenuItems(sub, item.submenu);
                        mi.appendChild(sub);
                    }
                    sub.style.display = "block";
                });
                mi.addEventListener("mouseleave", () => {
                    mi.style.background = ""; mi.style.color = "";
                    const sub = mi.querySelector(".perry-submenu");
                    if (sub) sub.style.display = "none";
                });
                container.appendChild(mi);
            } else {
                const mi = document.createElement("div");
                mi.style.cssText = "padding:4px 24px 4px 16px;cursor:pointer;display:flex;justify-content:space-between;white-space:nowrap;";
                const label = document.createElement("span");
                label.textContent = item.title;
                mi.appendChild(label);
                if (item.shortcut) {
                    const sc = document.createElement("span");
                    sc.textContent = item.shortcut;
                    sc.style.cssText = "margin-left:24px;color:#999;font-size:12px;";
                    mi.appendChild(sc);
                }
                mi.addEventListener("mouseenter", () => { mi.style.background = "#0066ff"; mi.style.color = "#fff"; });
                mi.addEventListener("mouseleave", () => { mi.style.background = ""; mi.style.color = ""; });
                mi.addEventListener("click", () => { dismissAll(); if (typeof item.callback === "function") item.callback(); });
                container.appendChild(mi);
            }
        }
    }

    for (const { title, menu_h } of bar.menus) {
        const titleEl = document.createElement("div");
        titleEl.textContent = title;
        titleEl.style.cssText = "padding:4px 10px;cursor:pointer;";
        titleEl.addEventListener("mouseenter", () => {
            titleEl.style.background = "#ddd";
            if (openDropdown && openTitle !== titleEl) {
                dismissAll();
                showDropdown(titleEl, menu_h);
            }
        });
        titleEl.addEventListener("mouseleave", () => { if (openTitle !== titleEl) titleEl.style.background = ""; });
        titleEl.addEventListener("click", (e) => {
            e.stopPropagation();
            if (openTitle === titleEl) { dismissAll(); titleEl.style.background = ""; return; }
            dismissAll();
            showDropdown(titleEl, menu_h);
        });
        barEl.appendChild(titleEl);

        function showDropdown(el, mh) {
            const dd = document.createElement("div");
            dd.style.cssText = "position:absolute;top:100%;background:#fff;border:1px solid #ccc;border-radius:4px;box-shadow:0 2px 8px rgba(0,0,0,0.15);padding:4px 0;min-width:180px;z-index:10001;";
            dd.style.left = el.offsetLeft + "px";
            renderMenuItems(dd, mh);
            barEl.appendChild(dd);
            openDropdown = dd;
            openTitle = el;
            el.style.background = "#ddd";
        }
    }

    document.addEventListener("click", () => {
        dismissAll();
        barEl.querySelectorAll(":scope > div").forEach(d => d.style.background = "");
    });

    // Insert at top of body, make body a flex column so menubar + root share viewport
    document.body.style.display = "flex";
    document.body.style.flexDirection = "column";
    barEl.style.flex = "0 0 auto";
    const root = document.getElementById("perry-root");
    if (root) root.style.flex = "1 1 0%";
    document.body.insertBefore(barEl, document.body.firstChild);
}

function perry_ui_widget_set_context_menu(widget_h, menu_h) {
    const el = getHandle(widget_h);
    const items = _menus.get(menu_h);
    if (!el || !items) return;
    el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        const menu = document.createElement("div");
        menu.style.cssText = "position:fixed;background:#fff;border:1px solid #ccc;border-radius:4px;box-shadow:0 2px 8px rgba(0,0,0,0.15);z-index:9999;padding:4px 0;";
        menu.style.left = e.clientX + "px"; menu.style.top = e.clientY + "px";
        for (const item of items) {
            if (item.type === "separator") {
                const sep = document.createElement("div");
                sep.style.cssText = "height:1px;background:#ccc;margin:4px 0;";
                menu.appendChild(sep);
                continue;
            }
            const mi = document.createElement("div");
            mi.textContent = item.title;
            mi.style.cssText = "padding:4px 16px;cursor:pointer;";
            mi.addEventListener("mouseenter", () => mi.style.background = "#f0f0f0");
            mi.addEventListener("mouseleave", () => mi.style.background = "");
            mi.addEventListener("click", () => { menu.remove(); if (typeof item.callback === "function") item.callback(); });
            menu.appendChild(mi);
        }
        document.body.appendChild(menu);
        const dismiss = () => { menu.remove(); document.removeEventListener("click", dismiss); };
        setTimeout(() => document.addEventListener("click", dismiss), 0);
    });
}

// --- Clipboard ---
function perry_ui_clipboard_read() {
    // Clipboard API is async; return empty for now
    return "";
}

function perry_ui_clipboard_write(text) {
    if (navigator.clipboard) navigator.clipboard.writeText(text);
}

// --- Dialogs ---
function perry_ui_open_file_dialog(callback) {
    const input = document.createElement("input");
    input.type = "file";
    input.addEventListener("change", () => {
        if (input.files.length > 0 && typeof callback === "function") callback(input.files[0].name);
    });
    input.click();
}

function perry_ui_save_file_dialog(callback, defaultName) {
    const name = prompt("Save as:", defaultName || "file.txt");
    if (name && typeof callback === "function") callback(name);
}

function perry_ui_poll_open_file() {
    // On web, file opening is handled asynchronously via openFolderDialog/openFileDialog
    return "";
}

function perry_ui_alert(title, message, buttons, callback) {
    const result = window.confirm(title + "\n\n" + message);
    if (typeof callback === "function") callback(result ? 0 : 1);
}

// --- Continuous keyboard events (issue #1864) ---
// Mirrors the native macOS contract: one document-level listener routes events
// to whichever widget owns focus, with a `0`-keyed fallback for app-level
// handlers. Names are normalised to perry-ui::keys canonical strings.
const _perryKeyDownCBs = new Map();
const _perryKeyUpCBs = new Map();
let _perryFocusedWidget = 0;
let _perryAppKeyDown = null;
let _perryAppKeyUp = null;
const _perryHeldKeys = new Set();
let _perryCurrentMods = 0;
let _perryKbdInstalled = false;
function _perryCodeToKey(code) {
    if (code.length === 4 && code.startsWith("Key")) {
        const c = code.charCodeAt(3);
        if (c >= 65 && c <= 90) return c - 64;
    }
    if (code.length === 6 && code.startsWith("Digit")) {
        const c = code.charCodeAt(5);
        if (c >= 48 && c <= 57) return c - 21;
    }
    if (code.length >= 2 && code.charAt(0) === "F") {
        const n = parseInt(code.slice(1), 10);
        if (n >= 1 && n <= 12) return 36 + n;
        if (n >= 13 && n <= 20) return 62 + n;
    }
    if (code.length >= 6 && code.startsWith("Numpad")) {
        const tail = code.slice(6);
        if (tail.length === 1) {
            const c = tail.charCodeAt(0);
            if (c >= 48 && c <= 57) return 83 + (c - 48);
        }
        switch (tail) {
            case "Decimal": return 93;
            case "Enter": return 94;
            case "Add": return 95;
            case "Subtract": return 96;
            case "Multiply": return 97;
            case "Divide": return 98;
            case "Equal": return 99;
            case "Clear": return 100;
        }
    }
    switch (code) {
        case "ArrowUp": return 49;
        case "ArrowDown": return 50;
        case "ArrowLeft": return 51;
        case "ArrowRight": return 52;
        case "Space": return 53;
        case "Enter": case "NumpadEnter": return 54;
        case "Tab": return 55;
        case "Escape": return 56;
        case "Backspace": return 57;
        case "Delete": return 58;
        case "Home": return 59;
        case "End": return 60;
        case "PageUp": return 61;
        case "PageDown": return 62;
        case "Insert": return 63;
        case "Minus": return 64;
        case "Equal": return 65;
        case "BracketLeft": return 66;
        case "BracketRight": return 67;
        case "Backslash": return 68;
        case "Semicolon": return 69;
        case "Quote": return 70;
        case "Comma": return 71;
        case "Period": return 72;
        case "Slash": return 73;
        case "Backquote": return 74;
        default: return 0;
    }
}
function _perryEventMods(e) {
    return (e.metaKey?1:0) | (e.shiftKey?2:0) | (e.altKey?4:0) | (e.ctrlKey?8:0);
}
function _perryEnsureKbd() {
    if (_perryKbdInstalled) return; _perryKbdInstalled = true;
    document.addEventListener("keydown", (e) => {
        _perryCurrentMods = _perryEventMods(e);
        const key = _perryCodeToKey(e.code); if (!key) return;
        _perryHeldKeys.add(key);
        const cb = _perryKeyDownCBs.get(_perryFocusedWidget) || _perryAppKeyDown;
        if (typeof cb === "function") cb(key, _perryCurrentMods, e.repeat);
    });
    document.addEventListener("keyup", (e) => {
        _perryCurrentMods = _perryEventMods(e);
        const key = _perryCodeToKey(e.code); if (!key) return;
        _perryHeldKeys.delete(key);
        const cb = _perryKeyUpCBs.get(_perryFocusedWidget) || _perryAppKeyUp;
        if (typeof cb === "function") cb(key, _perryCurrentMods);
    });
}
function perry_ui_widget_set_on_key_down(handle, cb) {
    _perryEnsureKbd();
    _perryKeyDownCBs.set(Number(handle), cb);
}
function perry_ui_widget_set_on_key_up(handle, cb) {
    _perryEnsureKbd();
    _perryKeyUpCBs.set(Number(handle), cb);
}
function perry_ui_app_set_on_key_down(cb) { _perryEnsureKbd(); _perryAppKeyDown = cb; }
function perry_ui_app_set_on_key_up(cb) { _perryEnsureKbd(); _perryAppKeyUp = cb; }
function perry_ui_focus_widget(handle) { _perryFocusedWidget = Number(handle); }
function perry_ui_blur_widget(handle) {
    if (_perryFocusedWidget === Number(handle)) _perryFocusedWidget = 0;
}
function perry_ui_is_key_down(keyCode) { _perryEnsureKbd(); return _perryHeldKeys.has(Number(keyCode)) ? 1 : 0; }
function perry_ui_current_modifiers() { _perryEnsureKbd(); return _perryCurrentMods; }

// --- Keyboard Shortcuts ---
function perry_ui_add_keyboard_shortcut(key, modifiers, callback) {
    if (typeof callback !== "function") return;
    document.addEventListener("keydown", (e) => {
        const wantMeta = (modifiers & 1) !== 0;
        const wantShift = (modifiers & 2) !== 0;
        const wantAlt = (modifiers & 4) !== 0;
        if (e.key.toLowerCase() === key.toLowerCase() &&
            e.metaKey === wantMeta && e.shiftKey === wantShift && e.altKey === wantAlt) {
            e.preventDefault(); callback();
        }
    });
}

// --- Sheet (Modal) ---
const _sheets = new Map();
let _nextSheetId = 1;

function perry_ui_sheet_create(width, height, title) {
    const id = _nextSheetId++;
    const overlay = document.createElement("div");
    overlay.style.cssText = "position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.4);display:none;z-index:2000;justify-content:center;align-items:center;";
    const sheet = document.createElement("div");
    sheet.style.cssText = `background:#fff;border-radius:12px;box-shadow:0 8px 32px rgba(0,0,0,0.25);width:${width}px;min-height:${height}px;padding:16px;`;
    overlay.appendChild(sheet);
    document.body.appendChild(overlay);
    _sheets.set(id, { overlay, sheet });
    return id;
}

function perry_ui_sheet_present(sheet_h) {
    const s = _sheets.get(sheet_h);
    if (s) s.overlay.style.display = "flex";
}

function perry_ui_sheet_dismiss(sheet_h) {
    const s = _sheets.get(sheet_h);
    if (s) s.overlay.style.display = "none";
}

// --- Toolbar ---
const _toolbars = new Map();
let _nextToolbarId = 1;

function perry_ui_toolbar_create() {
    const id = _nextToolbarId++;
    const bar = document.createElement("div");
    bar.style.cssText = "display:flex;gap:8px;padding:8px;background:#f5f5f5;border-bottom:1px solid #ddd;";
    _toolbars.set(id, bar);
    return id;
}

function perry_ui_toolbar_add_item(toolbar_h, label, icon, callback) {
    const bar = _toolbars.get(toolbar_h);
    if (!bar) return;
    const btn = document.createElement("button");
    btn.textContent = label || icon || "";
    btn.style.cssText = "padding:4px 12px;border:1px solid #ccc;border-radius:4px;background:#fff;cursor:pointer;font:inherit;";
    if (typeof callback === "function") btn.addEventListener("click", callback);
    bar.appendChild(btn);
}

function perry_ui_toolbar_attach(toolbar_h) {
    const bar = _toolbars.get(toolbar_h);
    if (bar) { const root = getRoot(); root.insertBefore(bar, root.firstChild); }
}

// --- System: Keychain (localStorage) ---
function perry_system_keychain_save(key, value) {
    localStorage.setItem("perry_keychain_" + key, value);
}

function perry_system_keychain_get(key) {
    return localStorage.getItem("perry_keychain_" + key) || "";
}

function perry_system_keychain_delete(key) {
    localStorage.removeItem("perry_keychain_" + key);
}

// --- System: Share Sheet (#917) ---
// Stub: navigator.share() is the obvious mapping but it's user-gesture-gated,
// available only over HTTPS, and partially supported. For matrix parity we
// no-op + log; native impls land per-platform separately.
function perry_system_share_text(text, _title) {
    if (typeof navigator !== "undefined" && typeof navigator.share === "function") {
        try { navigator.share({ text: String(text || "") }); return; } catch (_) {}
    }
    console.warn("perry/system: share_text not implemented on web (no navigator.share)");
}

function perry_system_share_url(url, _title) {
    if (typeof navigator !== "undefined" && typeof navigator.share === "function") {
        try { navigator.share({ url: String(url || "") }); return; } catch (_) {}
    }
    console.warn("perry/system: share_url not implemented on web (no navigator.share)");
}

// --- System: App Group / cross-process shared storage (#675) ---
// Stub: web has no cross-origin shared storage primitive. Back the API with
// localStorage under a stable prefix so within-origin reads/writes round-trip.
function perry_system_app_group_set(key, value) {
    localStorage.setItem("perry_appgroup_" + key, value);
}

function perry_system_app_group_get(key) {
    return localStorage.getItem("perry_appgroup_" + key) || "";
}

function perry_system_app_group_delete(key) {
    localStorage.removeItem("perry_appgroup_" + key);
}

// --- System: OS version (bug-report flow) ---
function perry_system_get_os_version() {
    if (typeof navigator !== "undefined" && navigator.userAgent) return navigator.userAgent;
    return "web";
}

// --- System: Notifications ---
function perry_system_notification_send(title, body) {
    if ("Notification" in window && Notification.permission === "granted") {
        new Notification(title, { body: body });
    } else if ("Notification" in window) {
        Notification.requestPermission().then(p => { if (p === "granted") new Notification(title, { body: body }); });
    }
}

// --- Run App ---
function perry_ui_app_run() {
    // In browser, the app is already "running" once DOM is ready.
    // This is a no-op.
}

// --- Timer Functions ---
function perry_set_timeout(callback, ms) {
    return setTimeout(callback, ms);
}

function perry_set_interval(callback, ms) {
    return setInterval(callback, ms);
}

function perry_clear_timeout(id) {
    clearTimeout(id);
}

function perry_clear_interval(id) {
    clearInterval(id);
}

// --- Path Helpers (simplified browser versions) ---
const __path = {
    join: function(...parts) {
        return parts.join("/").replace(/\/+/g, "/");
    },
    dirname: function(p) {
        const i = p.lastIndexOf("/");
        return i >= 0 ? p.substring(0, i) : ".";
    },
    basename: function(p) {
        const i = p.lastIndexOf("/");
        return i >= 0 ? p.substring(i + 1) : p;
    },
    extname: function(p) {
        const b = __path.basename(p);
        const i = b.lastIndexOf(".");
        return i > 0 ? b.substring(i) : "";
    },
    resolve: function(...parts) {
        return __path.join(...parts);
    },
    isAbsolute: function(p) {
        return p.startsWith("/");
    }
};

// ==========================================================================
// Hone Editor FFI — DOM-based implementations of the 28 native FFI functions
// ==========================================================================

const _honeEditors = new Map(); // handle → editor state
let _honeEditorNextHandle = 1;
let _honeEditorCSSInjected = false;

function _injectEditorCSS() {
    if (_honeEditorCSSInjected) return;
    _honeEditorCSSInjected = true;
    const s = document.createElement("style");
    s.textContent = `
.hone-editor { position: relative; overflow: hidden; contain: strict; outline: none;
  font-variant-ligatures: contextual; -webkit-font-smoothing: antialiased;
  cursor: text; background-color: #1e1e1e; color: #d4d4d4; }
.hone-editor-lines { position: absolute; top: 0; left: 0; right: 0; bottom: 0; }
.hone-editor-line { position: absolute; left: 0; right: 0; white-space: pre;
  pointer-events: none; }
.hone-editor-cursor-el { position: absolute; width: 2px; pointer-events: none;
  animation: hone-blink 1s step-end infinite; }
.hone-editor-selection-rect { position: absolute; pointer-events: none; }
.hone-editor-ghost { position: absolute; pointer-events: none; white-space: pre; }
.hone-editor-decoration { position: absolute; pointer-events: none; }
@keyframes hone-blink { 0%,100% { opacity: 1; } 50% { opacity: 0; } }
`;
    document.head.appendChild(s);
}

function hone_editor_create(width, height) {
    _injectEditorCSS();
    const h = _honeEditorNextHandle++;
    const root = document.createElement("div");
    root.className = "hone-editor";
    root.tabIndex = 0;
    root.style.width = width + "px";
    root.style.height = height + "px";
    root.style.flex = "1 1 0%";

    const linesContainer = document.createElement("div");
    linesContainer.className = "hone-editor-lines";
    root.appendChild(linesContainer);

    // Measurement canvas (hidden, for text metrics)
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d");

    // Cursor element
    const cursorEl = document.createElement("div");
    cursorEl.className = "hone-editor-cursor-el";
    cursorEl.style.background = "#d4d4d4";
    root.appendChild(cursorEl);

    // Selection container
    const selContainer = document.createElement("div");
    selContainer.style.position = "absolute";
    selContainer.style.top = "0";
    selContainer.style.left = "0";
    selContainer.style.right = "0";
    selContainer.style.bottom = "0";
    selContainer.style.pointerEvents = "none";
    root.insertBefore(selContainer, linesContainer);

    const editor = {
        root: root,
        linesContainer: linesContainer,
        canvas: canvas,
        ctx: ctx,
        cursorEl: cursorEl,
        selContainer: selContainer,
        linePool: [],       // reusable line <div> elements
        activeLines: {},    // lineNum → <div>
        fontFamily: "JetBrains Mono, Menlo, Monaco, Courier New, monospace",
        fontSize: 14,
        charWidth: 8.4,
        lineHeight: 21,
        gutterWidth: 0,
        scrollOffsetY: 0,
        pendingEvents: [],
        cursorEls: [cursorEl],
        ghostEl: null,
        batchMode: false,
    };

    // Set initial font on canvas for measurement
    ctx.font = editor.fontSize + "px " + editor.fontFamily;
    editor.charWidth = ctx.measureText("M").width;

    // Set up event listeners
    _setupEditorEvents(root, editor);

    _honeEditors.set(h, editor);
    // Also store in the handle system for embedNSView
    handles.set(h + 100000, root);
    return h;
}

function _setupEditorEvents(root, editor) {
    root.addEventListener("keydown", function(e) {
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey) {
            // Printable character → TEXT event
            editor.pendingEvents.push({ type: 1, char: e.key.charCodeAt(0), action: 0, x: 0, y: 0 });
        } else {
            var actionId = _mapKeyToAction(e.key, e.ctrlKey, e.shiftKey, e.metaKey, e.altKey);
            if (actionId > 0) {
                editor.pendingEvents.push({ type: 2, char: 0, action: actionId, x: 0, y: 0 });
            }
        }
        e.preventDefault();
    });

    root.addEventListener("wheel", function(e) {
        editor.pendingEvents.push({ type: 3, char: 0, action: 0, x: e.deltaX, y: e.deltaY });
        editor._scrollDelta = (editor._scrollDelta || 0) + e.deltaY;
        editor._needsLines = 1;
        e.preventDefault();
    }, { passive: false });

    root.addEventListener("mousedown", function(e) {
        var rect = root.getBoundingClientRect();
        editor.pendingEvents.push({ type: 4, char: 0, action: 0, x: e.clientX - rect.left, y: e.clientY - rect.top });
        root.focus();
    });

    // Handle paste
    root.addEventListener("paste", function(e) {
        var text = (e.clipboardData || window.clipboardData).getData("text");
        for (var i = 0; i < text.length; i++) {
            var ch = text.charCodeAt(i);
            if (ch === 10) {
                editor.pendingEvents.push({ type: 2, char: 0, action: 9, x: 0, y: 0 }); // Enter
            } else if (ch !== 13) {
                editor.pendingEvents.push({ type: 1, char: ch, action: 0, x: 0, y: 0 });
            }
        }
        e.preventDefault();
    });
}

function _mapKeyToAction(key, ctrl, shift, meta, alt) {
    // cmd/ctrl key combos
    var mod = meta || ctrl;
    if (key === "ArrowLeft" && shift) return 15;
    if (key === "ArrowRight" && shift) return 16;
    if (key === "ArrowUp" && shift) return 17;
    if (key === "ArrowDown" && shift) return 18;
    if (key === "ArrowLeft" && alt) return 13;
    if (key === "ArrowRight" && alt) return 14;
    if (key === "ArrowLeft") return 1;
    if (key === "ArrowRight") return 2;
    if (key === "ArrowUp") return 3;
    if (key === "ArrowDown") return 4;
    if (key === "Home" && shift) return 19;
    if (key === "End" && shift) return 20;
    if (key === "Home") return 5;
    if (key === "End") return 6;
    if (key === "Backspace" && alt) return 27;
    if (key === "Backspace") return 10;
    if (key === "Delete") return 11;
    if (key === "Enter") return 9;
    if (key === "Tab") return 12;
    if (key === "PageUp") return 28;
    if (key === "PageDown") return 29;
    if (mod && key === "a") return 21;
    if (mod && key === "x") return 22;
    if (mod && key === "c") return 23;
    if (mod && key === "v") return 24;
    if (mod && key === "z" && shift) return 26;
    if (mod && key === "z") return 25;
    return 0;
}

function hone_editor_destroy(h) {
    var ed = _honeEditors.get(h);
    if (ed && ed.root.parentNode) ed.root.parentNode.removeChild(ed.root);
    _honeEditors.delete(h);
    handles.delete(h + 100000);
}

function hone_editor_attach_to_view(h, parentId) {
    var ed = _honeEditors.get(h);
    var parent = getHandle(parentId);
    if (ed && parent) parent.appendChild(ed.root);
}

function hone_editor_set_font(h, family, size) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    if (typeof family === "string" && family.length > 0) {
        ed.fontFamily = family + ", Menlo, Monaco, Courier New, monospace";
    }
    ed.fontSize = size;
    ed.lineHeight = Math.round(size * 1.5);
    ed.ctx.font = size + "px " + ed.fontFamily;
    ed.charWidth = ed.ctx.measureText("M").width;
    ed.root.style.fontFamily = ed.fontFamily;
    ed.root.style.fontSize = size + "px";
    ed.root.style.lineHeight = ed.lineHeight + "px";
}

function hone_editor_measure_text(h, text) {
    var ed = _honeEditors.get(h);
    if (!ed) return 0;
    if (typeof text !== "string") return 0;
    return ed.ctx.measureText(text).width;
}

function hone_editor_render_line(h, lineNumber, text, tokensJson, yOffset) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    // Skip if using TS-authoritative viewport mode (cache_line + set_viewport handles rendering)
    if (ed._lineCache) return;

    var lineEl = ed.activeLines[lineNumber];
    if (!lineEl) {
        lineEl = ed.linePool.length > 0 ? ed.linePool.pop() : document.createElement("div");
        lineEl.className = "hone-editor-line";
        lineEl.style.fontFamily = ed.fontFamily;
        lineEl.style.fontSize = ed.fontSize + "px";
        lineEl.style.lineHeight = ed.lineHeight + "px";
        ed.linesContainer.appendChild(lineEl);
        ed.activeLines[lineNumber] = lineEl;
    }

    lineEl.style.top = yOffset + "px";
    lineEl.innerHTML = "";

    // Parse tokens JSON and render spans
    var tokens = null;
    if (typeof tokensJson === "string" && tokensJson.length > 2) {
        try { tokens = JSON.parse(tokensJson); } catch(e) {}
    }

    if (tokens && tokens.length > 0) {
        for (var i = 0; i < tokens.length; i++) {
            var tok = tokens[i];
            var span = document.createElement("span");
            if (tok.color) span.style.color = tok.color;
            if (tok.italic) span.style.fontStyle = "italic";
            if (tok.bold) span.style.fontWeight = "bold";
            var startPos = tok.start || 0;
            var endPos = tok.end || text.length;
            span.textContent = text.substring(startPos, endPos);
            lineEl.appendChild(span);
        }
    } else {
        lineEl.textContent = text;
    }
}

function hone_editor_set_cursor(h, x, y, cursorStyle) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    var el = ed.cursorEl;
    el.style.left = x + "px";
    el.style.top = y + "px";
    el.style.height = ed.lineHeight + "px";
    if (cursorStyle === 1) {
        // Block cursor
        el.style.width = ed.charWidth + "px";
        el.style.background = "rgba(212, 212, 212, 0.4)";
    } else if (cursorStyle === 2) {
        // Underline cursor
        el.style.width = ed.charWidth + "px";
        el.style.height = "2px";
        el.style.top = (y + ed.lineHeight - 2) + "px";
        el.style.background = "#d4d4d4";
    } else {
        // Line cursor (default)
        el.style.width = "2px";
        el.style.background = "#d4d4d4";
    }
}

function hone_editor_set_cursors(h, cursorsJson) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    var cursors = null;
    try { cursors = JSON.parse(cursorsJson); } catch(e) { return; }
    if (!cursors) return;

    // Ensure we have enough cursor elements
    while (ed.cursorEls.length < cursors.length) {
        var cel = document.createElement("div");
        cel.className = "hone-editor-cursor-el";
        cel.style.background = "#d4d4d4";
        ed.root.appendChild(cel);
        ed.cursorEls.push(cel);
    }
    // Hide extra cursors
    for (var i = cursors.length; i < ed.cursorEls.length; i++) {
        ed.cursorEls[i].style.display = "none";
    }
    // Position visible cursors
    for (var j = 0; j < cursors.length; j++) {
        var c = cursors[j];
        var el = ed.cursorEls[j];
        el.style.display = "";
        el.style.left = c.x + "px";
        el.style.top = c.y + "px";
        el.style.height = ed.lineHeight + "px";
        el.style.width = "2px";
    }
}

function hone_editor_set_selection(h, regionsJson) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    var regions = null;
    if (typeof regionsJson === "string" && regionsJson.length > 2) {
        try { regions = JSON.parse(regionsJson); } catch(e) {}
    }

    ed.selContainer.innerHTML = "";
    if (!regions || regions.length === 0) return;

    for (var i = 0; i < regions.length; i++) {
        var r = regions[i];
        var rect = document.createElement("div");
        rect.className = "hone-editor-selection-rect";
        rect.style.left = r.x + "px";
        rect.style.top = r.y + "px";
        rect.style.width = r.w + "px";
        rect.style.height = r.h + "px";
        rect.style.background = "rgba(38, 79, 122, 0.3)";
        ed.selContainer.appendChild(rect);
    }
}

function hone_editor_scroll(h, offsetY) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed.scrollOffsetY = offsetY;
    ed.linesContainer.style.transform = "translateY(" + (-offsetY) + "px)";
}

function hone_editor_begin_frame(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed.batchMode = true;
    // Clear stale lines at the start of a new frame
    for (var ln in ed.activeLines) {
        var lineEl = ed.activeLines[ln];
        lineEl.style.display = "none";
        ed.linePool.push(lineEl);
    }
    ed.activeLines = {};
}

function hone_editor_end_frame(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed.batchMode = false;
    // Show all active lines
    for (var ln in ed.activeLines) {
        ed.activeLines[ln].style.display = "";
    }
}

function hone_editor_invalidate(h) {
    // No-op on web — rendering is immediate via DOM manipulation
}

function hone_editor_render_ghost_text(h, text, x, y, color) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    if (!ed.ghostEl) {
        ed.ghostEl = document.createElement("div");
        ed.ghostEl.className = "hone-editor-ghost";
        ed.root.appendChild(ed.ghostEl);
    }
    ed.ghostEl.style.left = x + "px";
    ed.ghostEl.style.top = y + "px";
    ed.ghostEl.style.color = typeof color === "string" ? color : "rgba(128,128,128,0.5)";
    ed.ghostEl.style.fontFamily = ed.fontFamily;
    ed.ghostEl.style.fontSize = ed.fontSize + "px";
    ed.ghostEl.textContent = text;
    ed.ghostEl.style.display = "";
}

function hone_editor_render_decorations(h, decorationsJson) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    // Remove existing decorations
    var existing = ed.root.querySelectorAll(".hone-editor-decoration");
    for (var i = 0; i < existing.length; i++) existing[i].remove();

    var decorations = null;
    try { decorations = JSON.parse(decorationsJson); } catch(e) { return; }
    if (!decorations) return;

    for (var j = 0; j < decorations.length; j++) {
        var d = decorations[j];
        var el = document.createElement("div");
        el.className = "hone-editor-decoration";
        el.style.left = d.x + "px";
        el.style.top = d.y + "px";
        el.style.width = d.w + "px";
        el.style.height = d.h + "px";
        if (d.background) el.style.background = d.background;
        if (d.borderBottom) el.style.borderBottom = d.borderBottom;
        ed.root.appendChild(el);
    }
}

function hone_editor_set_gutter_width(h, width) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed.gutterWidth = width;
    ed.linesContainer.style.paddingLeft = width + "px";
}

function hone_editor_set_ts_mode(h, mode) {
    // On web, TypeScript always handles all state — this is a no-op
}

function hone_editor_set_event_callback(h, cb) {
    // Not used in Perry poll mode — no-op on web
}

function hone_editor_nsview(h) {
    // On web, return a handle ID that embed_ns_view can look up
    return h + 100000;
}

function hone_editor_pending_event_count(h) {
    var ed = _honeEditors.get(h);
    return ed ? ed.pendingEvents.length : 0;
}

function hone_editor_get_event_type(h, i) {
    var ed = _honeEditors.get(h);
    return (ed && i < ed.pendingEvents.length) ? ed.pendingEvents[i].type : 0;
}

function hone_editor_get_event_char(h, i) {
    var ed = _honeEditors.get(h);
    return (ed && i < ed.pendingEvents.length) ? ed.pendingEvents[i].char : 0;
}

function hone_editor_get_event_action(h, i) {
    var ed = _honeEditors.get(h);
    return (ed && i < ed.pendingEvents.length) ? ed.pendingEvents[i].action : 0;
}

function hone_editor_get_event_x(h, i) {
    var ed = _honeEditors.get(h);
    return (ed && i < ed.pendingEvents.length) ? ed.pendingEvents[i].x : 0;
}

function hone_editor_get_event_y(h, i) {
    var ed = _honeEditors.get(h);
    return (ed && i < ed.pendingEvents.length) ? ed.pendingEvents[i].y : 0;
}

function hone_editor_clear_events(h) {
    var ed = _honeEditors.get(h);
    if (ed) ed.pendingEvents.length = 0;
}

// --- TS-Authoritative Editor Protocol (cache_line, set_viewport, selections) ---

function hone_editor_cache_line(h, lineNumber, text, packedTokens) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    if (!ed._lineCache) ed._lineCache = {};
    ed._lineCache[lineNumber] = { text: text, tokens: packedTokens };
}

function hone_editor_set_viewport(h, startLine, endLine, scrollTop, totalLines, lineHeight) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed._vpStart = startLine;
    ed._vpEnd = endLine;
    ed._vpScrollTop = scrollTop;
    ed._vpTotalLines = totalLines;
    if (lineHeight > 0) ed.lineHeight = lineHeight;

    // Render visible lines from cache
    ed.linesContainer.innerHTML = "";
    ed.linesContainer.style.position = "relative";
    ed.linesContainer.style.height = (totalLines * ed.lineHeight) + "px";

    var cache = ed._lineCache || {};
    for (var line = startLine; line < endLine; line++) {
        var entry = cache[line];
        var div = document.createElement("div");
        div.className = "hone-editor-line";
        div.style.position = "absolute";
        div.style.top = ((line - startLine) * ed.lineHeight) + "px";
        div.style.left = ed.gutterWidth + "px";
        div.style.height = ed.lineHeight + "px";
        div.style.lineHeight = ed.lineHeight + "px";
        div.style.whiteSpace = "pre";
        div.style.fontFamily = ed.fontFamily;
        div.style.fontSize = ed.fontSize + "px";

        if (entry) {
            // Render with token coloring if we have packed tokens
            if (entry.tokens && entry.tokens.length > 0) {
                _renderTokenizedLine(div, entry.text, entry.tokens);
            } else {
                div.textContent = entry.text;
            }
        }

        // Line background color
        if (ed._lineBGs && ed._lineBGs[line]) {
            div.style.backgroundColor = ed._lineBGs[line];
        }

        ed.linesContainer.appendChild(div);
    }

    // Gutter
    if (ed.gutterWidth > 0) {
        for (var line = startLine; line < endLine; line++) {
            var gutter = document.createElement("div");
            gutter.style.position = "absolute";
            gutter.style.top = ((line - startLine) * ed.lineHeight) + "px";
            gutter.style.left = "0";
            gutter.style.width = ed.gutterWidth + "px";
            gutter.style.height = ed.lineHeight + "px";
            gutter.style.lineHeight = ed.lineHeight + "px";
            gutter.style.textAlign = "right";
            gutter.style.paddingRight = "8px";
            gutter.style.fontFamily = ed.fontFamily;
            gutter.style.fontSize = ed.fontSize + "px";
            gutter.style.color = ed._gutterFG || "#858585";
            gutter.style.userSelect = "none";
            gutter.textContent = String(line + 1);
            ed.linesContainer.appendChild(gutter);
        }
    }
}

function _renderTokenizedLine(div, text, packedTokens) {
    // Format: "startCol,endCol,hexColor,styleInt|..." with optional "BG:hexColor|" prefix
    var packed = packedTokens;

    // Handle BG prefix
    if (packed.length > 3 && packed.charAt(0) === "B" && packed.charAt(1) === "G" && packed.charAt(2) === ":") {
        var bgEnd = packed.indexOf("|");
        if (bgEnd > 3) {
            div.style.backgroundColor = "#" + packed.substring(3, bgEnd);
            packed = packed.substring(bgEnd + 1);
        }
    }

    if (packed.length < 3) {
        div.textContent = text;
        return;
    }

    // Parse pipe-separated tokens: startCol,endCol,hexColor,styleInt|
    var segments = packed.split("|");
    var lastEnd = 0;
    for (var t = 0; t < segments.length; t++) {
        var seg = segments[t];
        if (seg.length < 3) continue;
        var parts = seg.split(",");
        if (parts.length < 3) continue;
        var s = parseInt(parts[0]) || 0;
        var e = parseInt(parts[1]) || s;
        var hex = parts[2] || "d4d4d4";
        var style = parts.length > 3 ? parseInt(parts[3]) : 0;

        // Fill gap before this token
        if (s > lastEnd) {
            div.appendChild(document.createTextNode(text.substring(lastEnd, s)));
        }
        var span = document.createElement("span");
        span.style.color = "#" + hex;
        if (style === 1) span.style.fontStyle = "italic";
        if (style === 2) span.style.fontWeight = "bold";
        if (style === 3) { span.style.fontSize = "1.4em"; span.style.fontWeight = "bold"; }
        if (style === 4) { span.style.fontSize = "1.2em"; span.style.fontWeight = "bold"; }
        span.textContent = text.substring(s, e);
        div.appendChild(span);
        lastEnd = e;
    }
    if (lastEnd < text.length) {
        div.appendChild(document.createTextNode(text.substring(lastEnd)));
    }
}

function hone_editor_begin_selections(h, count) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    // Clear existing selection rects
    ed.selContainer.innerHTML = "";
}

function hone_editor_add_selection_rect(h, x, y, w, ht) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    var rect = document.createElement("div");
    rect.style.position = "absolute";
    rect.style.left = x + "px";
    rect.style.top = y + "px";
    rect.style.width = w + "px";
    rect.style.height = ht + "px";
    rect.style.backgroundColor = ed._selColor || "rgba(38,79,120,0.5)";
    rect.style.pointerEvents = "none";
    ed.selContainer.appendChild(rect);
}

function hone_editor_set_read_only(h, mode) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed._readOnly = mode;
}

function hone_editor_clear_line_cache(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed._lineCache = {};
}

function hone_editor_set_line_background(h, line, r, g, b, a) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    if (!ed._lineBGs) ed._lineBGs = {};
    ed._lineBGs[line] = "rgba(" + Math.round(r*255) + "," + Math.round(g*255) + "," + Math.round(b*255) + "," + a + ")";
}

function hone_editor_clear_line_backgrounds(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed._lineBGs = {};
}

function hone_editor_get_view_width(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return 800;
    return ed.root.clientWidth || 800;
}

function hone_editor_get_view_height(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return 600;
    return ed.root.clientHeight || 600;
}

function hone_editor_get_scroll_delta(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return 0;
    return ed._scrollDelta || 0;
}

function hone_editor_clear_scroll_delta(h) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed._scrollDelta = 0;
}

function hone_editor_needs_lines(h) {
    // Return 1 if the viewport has changed and TS needs to supply new cached lines
    var ed = _honeEditors.get(h);
    if (!ed) return 0;
    var needs = ed._needsLines || 0;
    ed._needsLines = 0;
    return needs;
}

function hone_editor_copy_to_clipboard(ctx, text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).catch(function() {});
    }
}

function hone_editor_set_bg_color(h, r, g, b) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed.root.style.backgroundColor = "rgb(" + Math.round(r*255) + "," + Math.round(g*255) + "," + Math.round(b*255) + ")";
}

function hone_editor_set_fg_color(h, r, g, b) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed.root.style.color = "rgb(" + Math.round(r*255) + "," + Math.round(g*255) + "," + Math.round(b*255) + ")";
}

function hone_editor_set_gutter_fg_color(h, r, g, b) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed._gutterFG = "rgb(" + Math.round(r*255) + "," + Math.round(g*255) + "," + Math.round(b*255) + ")";
}

function hone_editor_set_selection_color(h, r, g, b, a) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    ed._selColor = "rgba(" + Math.round(r*255) + "," + Math.round(g*255) + "," + Math.round(b*255) + "," + a + ")";
}

function hone_editor_set_cursor_color(h, r, g, b) {
    var ed = _honeEditors.get(h);
    if (!ed) return;
    var color = "rgb(" + Math.round(r*255) + "," + Math.round(g*255) + "," + Math.round(b*255) + ")";
    ed.cursorEl.style.background = color;
    for (var i = 0; i < ed.cursorEls.length; i++) {
        ed.cursorEls[i].style.background = color;
    }
}

// ==========================================================================
// Missing Widget Functions (used by hone-ide)
// ==========================================================================

function perry_ui_widget_remove_child(parent_h, child_h) {
    var parent = getHandle(parent_h);
    var child = getHandle(child_h);
    if (parent && child && child.parentNode === parent) parent.removeChild(child);
}

function perry_ui_widget_reorder_child(parent_h, fromIdx, toIdx) {
    var parent = getHandle(parent_h);
    if (!parent) return;
    var children = Array.from(parent.children);
    var from = Math.floor(fromIdx);
    var to = Math.floor(toIdx);
    if (from < 0 || from >= children.length) return;
    var child = children[from];
    parent.removeChild(child);
    if (to >= parent.children.length) {
        parent.appendChild(child);
    } else {
        parent.insertBefore(child, parent.children[to]);
    }
}

function perry_ui_widget_set_height(h, height) {
    var el = getHandle(h);
    if (el) { el.style.height = height + "px"; el.style.minHeight = height + "px"; }
}

function perry_ui_widget_match_parent_height(h) {
    var el = getHandle(h);
    if (el) { el.style.height = "100%"; el.style.flex = "1 1 0%"; }
}

function perry_ui_widget_match_parent_width(h) {
    var el = getHandle(h);
    if (el) { el.style.width = "100%"; el.style.alignSelf = "stretch"; }
}

function perry_ui_widget_add_overlay(parent_h, overlay_h) {
    var parent = getHandle(parent_h);
    var overlay = getHandle(overlay_h);
    if (parent && overlay) {
        if (getComputedStyle(parent).position === "static") parent.style.position = "relative";
        overlay.style.position = "absolute";
        overlay.style.zIndex = "10";
        parent.appendChild(overlay);
    }
}

function perry_ui_widget_set_overlay_frame(overlay_h, x, y, width, height) {
    var el = getHandle(overlay_h);
    if (!el) return;
    el.style.left = x + "px";
    el.style.top = y + "px";
    el.style.width = width + "px";
    el.style.height = height + "px";
}

function perry_ui_widget_set_edge_insets(h, top, right, bottom, left) {
    var el = getHandle(h);
    if (el) el.style.padding = top + "px " + right + "px " + bottom + "px " + left + "px";
}

function perry_ui_stack_set_detaches_hidden(h, detaches) {
    // On web, hidden elements with display:none are already detached from layout.
    // No action needed — CSS display:none is equivalent to NSStackView detachesHiddenViews.
}

function perry_ui_stack_set_distribution(h, distribution) {
    var el = getHandle(h);
    if (!el) return;
    // 0 = Fill, 1 = FillEqually, 2 = FillProportionally, 3 = EqualSpacing, 4 = EqualCentering
    if (distribution === 1) {
        // FillEqually: all children same size
        for (var i = 0; i < el.children.length; i++) {
            el.children[i].style.flex = "1 1 0%";
        }
    }
}

function perry_ui_stack_set_alignment(h, alignment) {
    var el = getHandle(h);
    if (!el) return;
    // Map NSStackView alignment to CSS
    if (alignment === 0) el.style.alignItems = "stretch";
    else if (alignment === 1) el.style.alignItems = "flex-start";
    else if (alignment === 2) el.style.alignItems = "center";
    else if (alignment === 3) el.style.alignItems = "flex-end";
}

function perry_ui_button_set_image_position(h, position) {
    var el = getHandle(h);
    if (!el) return;
    // 0 = no image, 1 = image only, 2 = image left, 3 = image right, etc.
    // On web, button already has text + icon — just adjust flex direction
    if (position === 1) {
        // Image only: hide text portion
        el.style.fontSize = "0";
        // The icon (if set via buttonSetImage) is in textContent — restore icon size
        el.style.display = "inline-flex";
        el.style.alignItems = "center";
        el.style.justifyContent = "center";
        el.style.fontSize = "inherit";
    }
}

function perry_ui_text_set_wraps(h, wraps) {
    var el = getHandle(h);
    if (!el) return;
    if (wraps) {
        el.style.whiteSpace = "normal";
        el.style.wordWrap = "break-word";
        el.style.overflowWrap = "break-word";
    } else {
        el.style.whiteSpace = "nowrap";
        el.style.overflow = "hidden";
        el.style.textOverflow = "ellipsis";
    }
}

function perry_ui_text_set_color(h, r, g, b, a) {
    var el = getHandle(h);
    if (el) el.style.color = "rgba(" + Math.round(r*255) + "," + Math.round(g*255) + "," + Math.round(b*255) + "," + a + ")";
}

function perry_ui_textfield_get_string(h) {
    var el = getHandle(h);
    return el ? (el.value || "") : "";
}

function perry_ui_textfield_blur_all() {
    if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
}

function perry_ui_textfield_set_on_submit(h, callback) {
    var el = getHandle(h);
    if (!el || typeof callback !== "function") return;
    el.addEventListener("keydown", function(e) {
        if (e.key === "Enter") { e.preventDefault(); callback(el.value || ""); }
    });
}

function perry_ui_textfield_set_on_focus(h, callback) {
    var el = getHandle(h);
    if (el && typeof callback === "function") el.addEventListener("focus", callback);
}

function perry_ui_textarea_create(placeholder, callback) {
    var el = document.createElement("textarea");
    el.placeholder = placeholder || "";
    el.style.resize = "vertical";
    el.style.fontFamily = "inherit";
    if (typeof callback === "function") {
        el.addEventListener("input", function() { callback(el.value); });
    }
    return wrapWidget(allocHandle(el));
}

function perry_ui_textarea_set_string(h, text) {
    var el = getHandle(h);
    if (el) el.value = text;
}

function perry_ui_textarea_get_string(h) {
    var el = getHandle(h);
    return el ? (el.value || "") : "";
}

// --- Frame Split (used by iPad/tablet layout, works as simple side-by-side on web) ---
function perry_ui_frame_split_create(leftWidth) {
    var el = document.createElement("div");
    el.style.display = "flex";
    el.style.flexDirection = "row";
    el.style.flex = "1 1 0%";
    el.style.overflow = "hidden";
    el._leftWidth = leftWidth || 280;
    return wrapWidget(allocHandle(el));
}

function perry_ui_frame_split_add_child(split_h, child_h) {
    var split = getHandle(split_h);
    var child = getHandle(child_h);
    if (!split || !child) return;
    var childCount = split.children.length;
    if (childCount === 0) {
        // First child = left panel
        child.style.width = split._leftWidth + "px";
        child.style.minWidth = split._leftWidth + "px";
        child.style.flex = "0 0 " + split._leftWidth + "px";
        child.style.overflow = "auto";
    } else {
        // Second child = right panel (fills rest)
        child.style.flex = "1 1 0%";
        child.style.overflow = "auto";
    }
    split.appendChild(child);
}

// --- Menu: Clear (remove all items from a menu) ---
function perry_ui_menu_clear(menu_h) {
    var items = _menus.get(menu_h);
    if (items) items.length = 0;
}

// --- Menu: Standard Action (maps to no-op on web, native selectors don't apply) ---
function perry_ui_menu_add_standard_action(menu_h, title, selector, shortcut) {
    // On web, standard selectors like "cut:", "copy:", "paste:" are handled by the browser.
    // Just add a normal menu item that does nothing (browser handles it natively).
    var items = _menus.get(menu_h);
    if (items) items.push({ type: "item", title: title, callback: function() {
        // Try to execute the browser command for common actions
        if (selector === "cut:") document.execCommand("cut");
        else if (selector === "copy:") document.execCommand("copy");
        else if (selector === "paste:") document.execCommand("paste");
        else if (selector === "selectAll:") document.execCommand("selectAll");
        else if (selector === "undo:") document.execCommand("undo");
        else if (selector === "redo:") document.execCommand("redo");
    }, shortcut: shortcut || undefined });
}

// ==========================================================================
// Terminal Emulator (web stub — DOM-based terminal without real PTY)
// ==========================================================================

var _honeTerminals = new Map();
var _honeTerminalNextHandle = 1;

function hone_terminal_open(rows, cols, shell, cwd) {
    var h = _honeTerminalNextHandle++;
    var root = document.createElement("div");
    root.className = "hone-terminal";
    root.tabIndex = 0;
    root.style.cssText = "font-family:JetBrains Mono,Menlo,Monaco,Courier New,monospace;font-size:13px;line-height:1.4;padding:8px;overflow:auto;background:#1e1e1e;color:#cccccc;white-space:pre-wrap;word-break:break-all;flex:1 1 0%;outline:none;";

    var output = document.createElement("div");
    root.appendChild(output);

    var inputLine = document.createElement("div");
    inputLine.style.display = "flex";
    var prompt = document.createElement("span");
    prompt.textContent = "$ ";
    prompt.style.color = "#6a9955";
    var input = document.createElement("span");
    input.contentEditable = "true";
    input.style.cssText = "outline:none;flex:1;color:#cccccc;caret-color:#d4d4d4;white-space:pre;";
    inputLine.appendChild(prompt);
    inputLine.appendChild(input);
    root.appendChild(inputLine);

    var term = {
        root: root,
        output: output,
        input: input,
        inputLine: inputLine,
        pendingOutput: "",
        bgColor: "#1e1e1e",
        fgColor: "#cccccc",
    };

    // Handle Enter key
    input.addEventListener("keydown", function(e) {
        if (e.key === "Enter") {
            e.preventDefault();
            var cmd = input.textContent || "";
            var line = document.createElement("div");
            line.textContent = "$ " + cmd;
            line.style.color = term.fgColor;
            output.appendChild(line);
            var result = document.createElement("div");
            result.textContent = "Command not available in web browser: " + cmd;
            result.style.color = "#cc6666";
            output.appendChild(result);
            input.textContent = "";
            root.scrollTop = root.scrollHeight;
        }
    });

    root.addEventListener("click", function() { input.focus(); });

    _honeTerminals.set(h, term);
    handles.set(h + 200000, root);
    return h;
}

function hone_terminal_nsview(h) { return h + 200000; }

function hone_terminal_poll(h) {
    var term = _honeTerminals.get(h);
    if (!term || !term.pendingOutput) return 0;
    return 1;
}

function hone_terminal_write(h, data) {
    var term = _honeTerminals.get(h);
    if (!term) return 0;
    var line = document.createElement("div");
    line.textContent = (typeof data === "string") ? data : "";
    term.output.appendChild(line);
    term.root.scrollTop = term.root.scrollHeight;
    return 0;
}

function hone_terminal_resize(h, rows, cols) {
    // No-op on web — CSS handles sizing
    return 0;
}

function hone_terminal_close(h) {
    var term = _honeTerminals.get(h);
    if (term && term.root.parentNode) term.root.parentNode.removeChild(term.root);
    _honeTerminals.delete(h);
    handles.delete(h + 200000);
    return 0;
}

function hone_terminal_set_bg_fg(h, bgR, bgG, bgB, fgR, fgG, fgB) {
    var term = _honeTerminals.get(h);
    if (!term) return;
    var bg = "rgb(" + Math.round(bgR*255) + "," + Math.round(bgG*255) + "," + Math.round(bgB*255) + ")";
    var fg = "rgb(" + Math.round(fgR*255) + "," + Math.round(fgG*255) + "," + Math.round(fgB*255) + ")";
    term.bgColor = bg;
    term.fgColor = fg;
    term.root.style.background = bg;
    term.root.style.color = fg;
}

function hone_terminal_live_set_theme(h, themeJson) {
    // Parse theme and apply colors
    var term = _honeTerminals.get(h);
    if (!term) return;
    try {
        var theme = JSON.parse(themeJson);
        if (theme.background) term.root.style.background = theme.background;
        if (theme.foreground) term.root.style.color = theme.foreground;
    } catch(e) {}
}

// ==========================================================================
// File System Extensions (write support + child_process stubs)
// ==========================================================================

function fs_writeFileSync(path, content) {
    if (!window.__perryFileCache) window.__perryFileCache = {};
    window.__perryFileCache[path] = { isDir: false, handle: null, content: String(content), children: null };
    // If we have a File System Access API handle, try to write back
    if (window.__perryDirHandle && window.__perryFileCache[path] && window.__perryFileCache[path].handle) {
        _writeFileAsync(path, content);
    }
}

async function _writeFileAsync(path, content) {
    try {
        var entry = window.__perryFileCache[path];
        if (entry && entry.handle && entry.handle.createWritable) {
            var writable = await entry.handle.createWritable();
            await writable.write(content);
            await writable.close();
        }
    } catch(e) { console.warn("Failed to write file:", path, e); }
}

function fs_mkdirSync(path) {
    if (!window.__perryFileCache) window.__perryFileCache = {};
    if (!window.__perryFileCache[path]) {
        window.__perryFileCache[path] = { isDir: true, handle: null, content: null, children: [] };
    }
}

function fs_unlinkSync(path) {
    if (window.__perryFileCache) delete window.__perryFileCache[path];
}

function fs_appendFileSync(path, content) {
    var existing = fs_readFileSync(path) || "";
    fs_writeFileSync(path, existing + String(content));
}

function child_process_execSync(cmd) {
    console.warn("execSync not available in browser:", cmd);
    return "";
}

function hone_get_documents_dir() {
    return "/documents";
}

function hone_get_app_files_dir() {
    return "/app-files";
}

// --- Platform FFI stubs (web implementations) ---
function perry_get_screen_width()  { return window.innerWidth; }
function perry_get_screen_height() { return window.innerHeight; }
function perry_get_scale_factor()  { return window.devicePixelRatio || 1; }
function perry_has_hardware_keyboard() { return true; }
function perry_get_platform() { return "web"; }
function perry_get_orientation() { return window.innerWidth > window.innerHeight ? "landscape" : "portrait"; }
function perry_get_device_idiom() { return 0; } // 0 = not a tablet/phone on web
function perry_on_layout_change(callback) {
    if (typeof callback === "function") {
        window.addEventListener("resize", function() { callback(); });
    }
}
function perry_on_resize(cb) {
    window.addEventListener("resize", function() { cb(window.innerWidth, window.innerHeight); });
}
function perry_on_orientation_change(cb) {
    if (window.matchMedia) {
        window.matchMedia("(orientation: portrait)").addEventListener("change", function(e) {
            cb(e.matches ? "portrait" : "landscape");
        });
    }
}

// --- Streaming (node-fetch replacement via Fetch API) ---
// Status: 0=connecting, 1=streaming, 2=done, 3=error
var _streamNextHandle = 1;
var _streams = {};

function stream_start(url, method, body, headersJson) {
    var h = _streamNextHandle++;
    var entry = { status: 0, lines: [], reader: null, done: false, error: null, buffer: "" };
    _streams[h] = entry;

    var headers = {};
    try {
        if (headersJson && headersJson.length > 2) headers = JSON.parse(headersJson);
    } catch (e) {}

    var opts = { method: method || "GET", headers: headers };
    if (method === "POST" && body && body.length > 0) {
        opts.body = body;
    }

    fetch(url, opts).then(function(response) {
        if (!response.ok) {
            entry.status = 3;
            entry.error = "HTTP " + response.status;
            entry.lines.push("HTTP error: " + response.status);
            return;
        }
        entry.status = 1;
        var reader = response.body.getReader();
        entry.reader = reader;
        var decoder = new TextDecoder();

        function pump() {
            reader.read().then(function(result) {
                if (result.done) {
                    // Flush remaining buffer
                    if (entry.buffer.length > 0) {
                        entry.lines.push(entry.buffer);
                        entry.buffer = "";
                    }
                    entry.status = 2;
                    entry.done = true;
                    return;
                }
                var text = decoder.decode(result.value, { stream: true });
                entry.buffer += text;
                // Split by newlines, keep partial last line in buffer
                var parts = entry.buffer.split("\n");
                for (var i = 0; i < parts.length - 1; i++) {
                    entry.lines.push(parts[i] + "\n");
                }
                entry.buffer = parts[parts.length - 1];
                pump();
            }).catch(function(err) {
                if (entry.buffer.length > 0) {
                    entry.lines.push(entry.buffer);
                    entry.buffer = "";
                }
                entry.status = 3;
                entry.error = String(err);
            });
        }
        pump();
    }).catch(function(err) {
        entry.status = 3;
        entry.error = String(err);
    });

    return h;
}

function stream_poll(handle) {
    var entry = _streams[handle];
    if (!entry) return "";
    if (entry.lines.length > 0) {
        return entry.lines.shift();
    }
    return "";
}

function stream_status(handle) {
    var entry = _streams[handle];
    if (!entry) return 3;
    return entry.status;
}

function stream_close(handle) {
    var entry = _streams[handle];
    if (!entry) return;
    if (entry.reader) {
        try { entry.reader.cancel(); } catch (e) {}
    }
    delete _streams[handle];
}

// --- File System Web Cache ---
// Files are cached when a folder is opened via perry_ui_open_folder_dialog.
// readFileSync/readdirSync/isDirectory serve from this cache.
if (!window.__perryFileCache) window.__perryFileCache = {};

function fs_readFileSync(path) {
    const entry = window.__perryFileCache[path];
    if (entry && entry.content !== null) return entry.content;
    if (entry && entry.handle && entry.handle.getFile) {
        // Async read - but we need sync. Return empty and prefetch.
        _prefetchFile(path, entry.handle);
        return entry.content || "";
    }
    return "";
}

function fs_readdirSync(path) {
    var entry = window.__perryFileCache && window.__perryFileCache[path];
    if (entry && entry.children) return entry.children;
    return [];
}

function fs_isDirectory(path) {
    const entry = window.__perryFileCache[path];
    if (entry) return entry.isDir ? 1 : 0;
    return 0;
}

function fs_existsSync(path) {
    return window.__perryFileCache[path] ? true : false;
}

// Pre-fetch file content asynchronously and cache it
async function _prefetchFile(path, handle) {
    try {
        const file = await handle.getFile();
        const text = await file.text();
        if (window.__perryFileCache[path]) {
            window.__perryFileCache[path].content = text;
        }
    } catch(e) {
        console.warn("Failed to read file:", path, e);
    }
}

// Pre-fetch all files in the cache that haven't been loaded yet
async function _prefetchAllFiles() {
    for (const path in window.__perryFileCache) {
        const entry = window.__perryFileCache[path];
        if (!entry.isDir && entry.content === null && entry.handle) {
            await _prefetchFile(path, entry.handle);
        }
    }
}

// --- perry/media — HTML5 <audio> + Media Session API ---
//
// Per-process player table. Index 0 is reserved (handles are 1-based to
// match the dispatch-table contract — `0` means "no player / stub backend").
// Each entry: { audio, state, duration, hasStarted, ended, onStateChange?, onTimeUpdate? }.
const PERRY_MEDIA_PLAYERS = [];
let PERRY_MEDIA_SESSION_WIRED = false;

function _perry_media_get(handle) {
    if (typeof handle !== "number" || handle < 1) return null;
    return PERRY_MEDIA_PLAYERS[handle - 1] || null;
}

function _perry_media_flush_state(handle) {
    const e = _perry_media_get(handle);
    if (!e || typeof e.onStateChange !== "function") return;
    try { e.onStateChange(e.state); } catch (err) { console.warn("perry/media onStateChange threw:", err); }
}

function _perry_media_flush_time(handle) {
    const e = _perry_media_get(handle);
    if (!e || typeof e.onTimeUpdate !== "function") return;
    const t = isFinite(e.audio.currentTime) ? e.audio.currentTime : 0;
    const d = isFinite(e.duration) ? e.duration : 0;
    try { e.onTimeUpdate(t, d); } catch (err) { console.warn("perry/media onTimeUpdate threw:", err); }
}

function _perry_media_first_live_handle() {
    for (let i = 0; i < PERRY_MEDIA_PLAYERS.length; i++) {
        if (PERRY_MEDIA_PLAYERS[i]) return i + 1;
    }
    return 0;
}

function _perry_media_wire_session() {
    if (PERRY_MEDIA_SESSION_WIRED) return;
    if (!('mediaSession' in navigator)) return;
    PERRY_MEDIA_SESSION_WIRED = true;
    try {
        navigator.mediaSession.setActionHandler('play', () => {
            const h = _perry_media_first_live_handle();
            if (h) perry_media_play(h);
        });
        navigator.mediaSession.setActionHandler('pause', () => {
            const h = _perry_media_first_live_handle();
            if (h) perry_media_pause(h);
        });
        navigator.mediaSession.setActionHandler('seekto', (details) => {
            const h = _perry_media_first_live_handle();
            if (h && typeof details.seekTime === "number") perry_media_seek(h, details.seekTime);
        });
        navigator.mediaSession.setActionHandler('seekforward', (details) => {
            const h = _perry_media_first_live_handle();
            if (!h) return;
            const e = _perry_media_get(h);
            if (!e) return;
            const skip = (details && details.seekOffset) || 10;
            perry_media_seek(h, (e.audio.currentTime || 0) + skip);
        });
        navigator.mediaSession.setActionHandler('seekbackward', (details) => {
            const h = _perry_media_first_live_handle();
            if (!h) return;
            const e = _perry_media_get(h);
            if (!e) return;
            const skip = (details && details.seekOffset) || 10;
            perry_media_seek(h, Math.max(0, (e.audio.currentTime || 0) - skip));
        });
    } catch (err) { /* setActionHandler can throw on unsupported actions — ignore */ }
}

function perry_media_create_player(url) {
    const audio = new Audio(typeof url === "string" ? url : "");
    audio.preload = "auto";
    const entry = { audio, state: "loading", duration: 0, hasStarted: false, ended: false };
    PERRY_MEDIA_PLAYERS.push(entry);
    const handle = PERRY_MEDIA_PLAYERS.length; // 1-based
    audio.addEventListener('loadedmetadata', () => {
        entry.duration = isFinite(audio.duration) ? audio.duration : 0;
    });
    audio.addEventListener('canplay', () => {
        if (entry.state === 'loading') entry.state = 'ready';
        _perry_media_flush_state(handle);
    });
    audio.addEventListener('play', () => {
        entry.state = 'playing';
        _perry_media_flush_state(handle);
    });
    audio.addEventListener('playing', () => {
        entry.state = 'playing';
        _perry_media_flush_state(handle);
    });
    audio.addEventListener('pause', () => {
        if (entry.ended) return;
        entry.state = entry.hasStarted ? 'paused' : 'ready';
        _perry_media_flush_state(handle);
    });
    audio.addEventListener('ended', () => {
        entry.ended = true;
        entry.state = 'ended';
        _perry_media_flush_state(handle);
    });
    audio.addEventListener('error', () => {
        entry.state = 'error';
        _perry_media_flush_state(handle);
    });
    audio.addEventListener('waiting', () => {
        entry.state = 'loading';
        _perry_media_flush_state(handle);
    });
    audio.addEventListener('timeupdate', () => {
        // Belt-and-braces ended fallback (acroyear #351 comment) — Chromium
        // / Chromecast have historically dropped the `ended` event.
        if (entry.hasStarted && entry.duration > 0.25 && audio.currentTime >= entry.duration - 0.25 && !entry.ended) {
            entry.ended = true;
            entry.state = 'ended';
            _perry_media_flush_state(handle);
        }
        if (entry.state === 'playing' || entry.state === 'loading') {
            _perry_media_flush_time(handle);
        }
    });
    return handle;
}

function perry_media_play(handle) {
    const e = _perry_media_get(handle);
    if (!e) return;
    e.hasStarted = true;
    e.ended = false;
    const p = e.audio.play();
    if (p && typeof p.catch === "function") {
        p.catch(() => { e.state = 'error'; _perry_media_flush_state(handle); });
    }
}

function perry_media_pause(handle) {
    const e = _perry_media_get(handle);
    if (!e) return;
    e.audio.pause();
}

function perry_media_stop(handle) {
    const e = _perry_media_get(handle);
    if (!e) return;
    e.audio.pause();
    try { e.audio.currentTime = 0; } catch (_) { /* live stream — currentTime not settable */ }
    e.hasStarted = false;
    e.ended = false;
    e.state = 'ready';
    _perry_media_flush_state(handle);
}

function perry_media_seek(handle, seconds) {
    const e = _perry_media_get(handle);
    if (!e) return;
    const d = e.audio.duration;
    let target = Number(seconds) || 0;
    if (target < 0) target = 0;
    if (isFinite(d) && target > d) target = d;
    try { e.audio.currentTime = target; } catch (_) { /* live stream */ }
}

function perry_media_set_volume(handle, vol) {
    const e = _perry_media_get(handle);
    if (!e) return;
    let v = Number(vol);
    if (!isFinite(v)) v = 1;
    if (v < 0) v = 0;
    if (v > 1) v = 1;
    e.audio.volume = v;
}

function perry_media_set_rate(handle, rate) {
    const e = _perry_media_get(handle);
    if (!e) return;
    const r = Number(rate);
    if (!isFinite(r) || r <= 0) return;
    // Browsers clamp to ~[0.0625, 16] internally — passing anything sane is fine.
    e.audio.playbackRate = r;
}

function perry_media_get_current_time(handle) {
    const e = _perry_media_get(handle);
    if (!e) return 0;
    return isFinite(e.audio.currentTime) ? e.audio.currentTime : 0;
}

function perry_media_get_duration(handle) {
    const e = _perry_media_get(handle);
    if (!e) return 0;
    return isFinite(e.duration) ? e.duration : 0;
}

function perry_media_get_state(handle) {
    const e = _perry_media_get(handle);
    return e ? e.state : "error";
}

function perry_media_is_playing(handle) {
    const e = _perry_media_get(handle);
    return (e && e.state === 'playing') ? 1 : 0;
}

function perry_media_on_state_change(handle, callback) {
    const e = _perry_media_get(handle);
    if (!e) return;
    e.onStateChange = (typeof callback === "function") ? callback : null;
}

function perry_media_on_time_update(handle, callback) {
    const e = _perry_media_get(handle);
    if (!e) return;
    e.onTimeUpdate = (typeof callback === "function") ? callback : null;
}

function perry_media_set_now_playing(handle, title, artist, album, artworkUrl) {
    const e = _perry_media_get(handle);
    if (!e) return;
    if (!('mediaSession' in navigator)) return;
    try {
        const meta = {
            title: String(title || ""),
            artist: String(artist || ""),
            album: String(album || ""),
        };
        if (artworkUrl && typeof MediaMetadata !== "undefined") {
            meta.artwork = [{ src: String(artworkUrl) }];
        }
        navigator.mediaSession.metadata = (typeof MediaMetadata !== "undefined")
            ? new MediaMetadata(meta)
            : meta;
        _perry_media_wire_session();
    } catch (err) { console.warn("perry/media setNowPlaying:", err); }
}

function perry_media_destroy(handle) {
    const e = _perry_media_get(handle);
    if (!e) return;
    try { e.audio.pause(); } catch (_) {}
    try { e.audio.src = ""; } catch (_) {}
    e.onStateChange = null;
    e.onTimeUpdate = null;
    PERRY_MEDIA_PLAYERS[handle - 1] = null;
}

// --- Expose API ---
window.__perry = {
    // Handle system
    allocHandle, getHandle,
    // State
    stateCreate, stateGet, stateSet, stateSubscribe,
    // UI widgets
    perry_ui_app_create,
    perry_ui_vstack_create,
    perry_ui_hstack_create,
    perry_ui_zstack_create,
    perry_ui_text_create,
    perry_ui_button_create,
    perry_ui_textfield_create,
    perry_ui_securefield_create,
    perry_ui_toggle_create,
    perry_ui_toggle_set_state,
    perry_ui_slider_create,
    perry_ui_scrollview_create,
    perry_ui_spacer_create,
    perry_ui_divider_create,
    perry_ui_progressview_create,
    perry_ui_image_create,
    perry_ui_picker_create,
    perry_ui_form_create,
    perry_ui_section_create,
    perry_ui_navigationstack_create,
    perry_ui_canvas_create,
    perry_ui_lazyvstack_create,
    perry_ui_lazyvstack_update,
    perry_ui_table_create,
    perry_ui_table_set_column_header,
    perry_ui_table_set_column_width,
    perry_ui_table_update_row_count,
    perry_ui_table_set_on_row_select,
    perry_ui_table_get_selected_row,
    // Child management
    perry_ui_widget_add_child,
    perry_ui_widget_add_child_at,
    perry_ui_widget_remove_all_children,
    // Styling
    perry_ui_set_background,
    perry_ui_set_foreground,
    perry_ui_set_font_size,
    perry_ui_set_font_weight,
    perry_ui_set_font_family,
    perry_ui_set_padding,
    perry_ui_set_frame,
    perry_ui_set_corner_radius,
    perry_ui_set_border,
    perry_ui_set_opacity,
    perry_ui_set_enabled,
    perry_ui_set_tooltip,
    perry_ui_set_control_size,
    perry_ui_set_widget_hidden,
    perry_ui_widget_set_background_gradient,
    perry_ui_widget_set_context_menu,
    // Animations
    perry_ui_animate_opacity,
    perry_ui_animate_position,
    // Events
    perry_ui_set_on_click,
    perry_ui_set_on_hover,
    perry_ui_set_on_double_click,
    // State system
    perry_ui_state_create,
    perry_ui_state_get,
    perry_ui_state_set,
    perry_ui_state_bind_textfield,
    // State bindings
    perry_ui_state_bind_text,
    perry_ui_state_bind_text_numeric,
    perry_ui_state_bind_slider,
    perry_ui_state_bind_toggle,
    perry_ui_state_bind_visibility,
    perry_ui_state_bind_foreach,
    perry_ui_state_on_change,
    // Keyed state + setText (issue #1392 — state_desugar synthetic API)
    perry_ui_set_text,
    perry_ui_state_init,
    perry_ui_state_get_keyed,
    perry_ui_state_set_keyed,
    perry_ui_foreach_register,
    perry_ui_navstack_register_route,
    // Text / Button / TextField ops
    perry_ui_text_set_string,
    perry_ui_text_set_selectable,
    perry_ui_button_set_bordered,
    perry_ui_button_set_title,
    perry_ui_button_set_text_color,
    perry_ui_button_set_image,
    perry_ui_button_set_content_tint_color,
    perry_ui_textfield_focus,
    perry_ui_textfield_set_string,
    // ScrollView
    perry_ui_scrollview_set_child,
    perry_ui_scrollview_scroll_to,
    perry_ui_scrollview_get_offset,
    perry_ui_scrollview_set_offset,
    // Layout
    perry_ui_vstack_create_with_insets,
    perry_ui_hstack_create_with_insets,
    // Navigation
    perry_ui_navstack_push,
    perry_ui_navstack_pop,
    // Picker
    perry_ui_picker_add_item,
    perry_ui_picker_set_selected,
    perry_ui_picker_get_selected,
    // Image
    perry_ui_image_create_symbol,
    perry_ui_image_create_url,
    perry_ui_load_image,
    perry_ui_image_set_size,
    perry_ui_image_set_tint,
    // ProgressView
    perry_ui_progressview_set_value,
    // System
    perry_system_open_url,
    perry_system_is_dark_mode,
    perry_system_preferences_get,
    perry_system_preferences_set,
    perry_system_haptic_play,
    perry_system_keychain_save,
    perry_system_keychain_get,
    perry_system_keychain_delete,
    perry_system_notification_send,
    // Audio
    perry_system_audio_start,
    perry_system_audio_stop,
    perry_system_audio_get_level,
    perry_system_audio_get_peak,
    perry_system_audio_get_waveform,
    perry_system_get_device_model,
    // Media (perry/media — HTML5 audio + Media Session API)
    perry_media_create_player,
    perry_media_play,
    perry_media_pause,
    perry_media_stop,
    perry_media_seek,
    perry_media_set_volume,
    perry_media_set_rate,
    perry_media_get_current_time,
    perry_media_get_duration,
    perry_media_get_state,
    perry_media_is_playing,
    perry_media_on_state_change,
    perry_media_on_time_update,
    perry_media_set_now_playing,
    perry_media_destroy,
    // Canvas
    perry_ui_canvas_fill_rect,
    perry_ui_canvas_stroke_rect,
    perry_ui_canvas_clear_rect,
    perry_ui_canvas_set_fill_color,
    perry_ui_canvas_set_stroke_color,
    perry_ui_canvas_begin_path,
    perry_ui_canvas_move_to,
    perry_ui_canvas_line_to,
    perry_ui_canvas_arc,
    perry_ui_canvas_close_path,
    perry_ui_canvas_fill,
    perry_ui_canvas_stroke,
    perry_ui_canvas_set_line_width,
    perry_ui_canvas_fill_text,
    perry_ui_canvas_set_font,
    perry_ui_canvas_draw_image,
    perry_ui_canvas_fill_gradient,
    // Menu
    perry_ui_menu_create,
    perry_ui_menu_add_item,
    perry_ui_menu_add_item_with_shortcut,
    perry_ui_menu_add_separator,
    perry_ui_menu_add_submenu,
    perry_ui_menubar_create,
    perry_ui_menubar_add_menu,
    perry_ui_menubar_attach,
    // Clipboard
    perry_ui_clipboard_read,
    perry_ui_clipboard_write,
    // Dialogs
    perry_ui_open_file_dialog,
    perry_ui_open_folder_dialog,
    perry_ui_save_file_dialog,
    perry_ui_poll_open_file,
    perry_ui_alert,
    // Keyboard
    perry_ui_add_keyboard_shortcut,
    perry_ui_widget_set_on_key_down,
    perry_ui_widget_set_on_key_up,
    perry_ui_app_set_on_key_down,
    perry_ui_app_set_on_key_up,
    perry_ui_focus_widget,
    perry_ui_blur_widget,
    perry_ui_is_key_down,
    perry_ui_current_modifiers,
    // Sheets
    perry_ui_sheet_create,
    perry_ui_sheet_present,
    perry_ui_sheet_dismiss,
    // Toolbar
    perry_ui_toolbar_create,
    perry_ui_toolbar_add_item,
    perry_ui_toolbar_attach,
    // Windows
    perry_ui_window_create,
    perry_ui_window_set_body,
    perry_ui_window_show,
    perry_ui_window_close,
    // App lifecycle
    perry_ui_app_run,
    perry_ui_app_set_body,
    perry_ui_app_set_min_size,
    perry_ui_app_set_max_size,
    perry_ui_app_on_activate,
    perry_ui_app_on_terminate,
    perry_ui_app_set_timer,
    // Widget layout
    perry_ui_widget_set_width,
    perry_ui_widget_set_height,
    perry_ui_widget_set_hugging,
    perry_ui_widget_remove_child,
    perry_ui_widget_reorder_child,
    perry_ui_widget_match_parent_height,
    perry_ui_widget_match_parent_width,
    perry_ui_widget_add_overlay,
    perry_ui_widget_set_overlay_frame,
    perry_ui_widget_set_edge_insets,
    perry_ui_stack_set_detaches_hidden,
    perry_ui_stack_set_distribution,
    perry_ui_stack_set_alignment,
    perry_ui_button_set_image_position,
    perry_ui_text_set_wraps,
    perry_ui_text_set_color,
    perry_ui_textfield_get_string,
    perry_ui_textfield_blur_all,
    perry_ui_textfield_set_on_submit,
    perry_ui_textfield_set_on_focus,
    perry_ui_textarea_create,
    perry_ui_textarea_set_string,
    perry_ui_textarea_get_string,
    perry_ui_frame_split_create,
    perry_ui_frame_split_add_child,
    perry_ui_menu_add_standard_action,
    perry_ui_menu_clear,
    perry_ui_embed_ns_view,
    // Timers
    perry_set_timeout,
    perry_set_interval,
    perry_clear_timeout,
    perry_clear_interval,
    // Path
    path: __path,
    // Platform FFI
    perry_get_screen_width,
    perry_get_screen_height,
    perry_get_scale_factor,
    perry_has_hardware_keyboard,
    perry_get_platform,
    perry_get_orientation,
    perry_on_resize,
    perry_on_orientation_change,
    // File System (web cache)
    fs_readFileSync,
    fs_readdirSync,
    fs_isDirectory,
    fs_existsSync,
    fs_writeFileSync,
    fs_mkdirSync,
    fs_unlinkSync,
    fs_appendFileSync,
    // child_process
    child_process_execSync,
    // Hone app dirs
    hone_get_documents_dir,
    hone_get_app_files_dir,
    // Streaming (node-fetch replacement)
    stream_start,
    stream_poll,
    stream_status,
    stream_close,
};

// Expose platform FFI functions as globals (compiled code calls them as bare function names)
window.perry_get_screen_width = perry_get_screen_width;
window.perry_get_screen_height = perry_get_screen_height;
window.perry_get_scale_factor = perry_get_scale_factor;
window.perry_has_hardware_keyboard = perry_has_hardware_keyboard;
window.perry_get_platform = perry_get_platform;
window.perry_get_orientation = perry_get_orientation;
window.perry_get_device_idiom = perry_get_device_idiom;
window.perry_on_layout_change = perry_on_layout_change;
window.perry_on_resize = perry_on_resize;
window.perry_on_orientation_change = perry_on_orientation_change;
window.hone_get_documents_dir = hone_get_documents_dir;
window.hone_get_app_files_dir = hone_get_app_files_dir;

// Expose editor FFI functions as globals
window.hone_editor_create = hone_editor_create;
window.hone_editor_destroy = hone_editor_destroy;
window.hone_editor_attach_to_view = hone_editor_attach_to_view;
window.hone_editor_set_font = hone_editor_set_font;
window.hone_editor_measure_text = hone_editor_measure_text;
window.hone_editor_render_line = hone_editor_render_line;
window.hone_editor_set_cursor = hone_editor_set_cursor;
window.hone_editor_set_cursors = hone_editor_set_cursors;
window.hone_editor_set_selection = hone_editor_set_selection;
window.hone_editor_scroll = hone_editor_scroll;
window.hone_editor_begin_frame = hone_editor_begin_frame;
window.hone_editor_end_frame = hone_editor_end_frame;
window.hone_editor_invalidate = hone_editor_invalidate;
window.hone_editor_render_ghost_text = hone_editor_render_ghost_text;
window.hone_editor_render_decorations = hone_editor_render_decorations;
window.hone_editor_set_gutter_width = hone_editor_set_gutter_width;
window.hone_editor_set_ts_mode = hone_editor_set_ts_mode;
window.hone_editor_set_event_callback = hone_editor_set_event_callback;
window.hone_editor_nsview = hone_editor_nsview;
window.hone_editor_pending_event_count = hone_editor_pending_event_count;
window.hone_editor_get_event_type = hone_editor_get_event_type;
window.hone_editor_get_event_char = hone_editor_get_event_char;
window.hone_editor_get_event_action = hone_editor_get_event_action;
window.hone_editor_get_event_x = hone_editor_get_event_x;
window.hone_editor_get_event_y = hone_editor_get_event_y;
window.hone_editor_clear_events = hone_editor_clear_events;
window.hone_editor_cache_line = hone_editor_cache_line;
window.hone_editor_set_viewport = hone_editor_set_viewport;
window.hone_editor_begin_selections = hone_editor_begin_selections;
window.hone_editor_add_selection_rect = hone_editor_add_selection_rect;
window.hone_editor_set_read_only = hone_editor_set_read_only;
window.hone_editor_clear_line_cache = hone_editor_clear_line_cache;
window.hone_editor_set_line_background = hone_editor_set_line_background;
window.hone_editor_clear_line_backgrounds = hone_editor_clear_line_backgrounds;
window.hone_editor_get_view_width = hone_editor_get_view_width;
window.hone_editor_get_view_height = hone_editor_get_view_height;
window.hone_editor_get_scroll_delta = hone_editor_get_scroll_delta;
window.hone_editor_clear_scroll_delta = hone_editor_clear_scroll_delta;
window.hone_editor_needs_lines = hone_editor_needs_lines;
window.hone_editor_copy_to_clipboard = hone_editor_copy_to_clipboard;
window.hone_editor_set_bg_color = hone_editor_set_bg_color;
window.hone_editor_set_fg_color = hone_editor_set_fg_color;
window.hone_editor_set_gutter_fg_color = hone_editor_set_gutter_fg_color;
window.hone_editor_set_selection_color = hone_editor_set_selection_color;
window.hone_editor_set_cursor_color = hone_editor_set_cursor_color;

// Expose terminal FFI functions as globals
window.hone_terminal_open = hone_terminal_open;
window.hone_terminal_nsview = hone_terminal_nsview;
window.hone_terminal_poll = hone_terminal_poll;
window.hone_terminal_write = hone_terminal_write;
window.hone_terminal_resize = hone_terminal_resize;
window.hone_terminal_close = hone_terminal_close;
window.hone_terminal_set_bg_fg = hone_terminal_set_bg_fg;
window.hone_terminal_live_set_theme = hone_terminal_live_set_theme;

// --- Pre-populate demo project for web ---
(function() {
    if (!window.__perryFileCache) window.__perryFileCache = {};
    var C = window.__perryFileCache;
    function dir(p) { C[p] = { isDir: true, handle: null, content: null, children: null }; }
    function file(p, txt) { C[p] = { isDir: false, handle: null, content: txt, children: null }; }

    dir("/documents");
    dir("/documents/src");
    dir("/documents/src/components");
    dir("/documents/src/utils");
    dir("/documents/tests");

    file("/documents/README.md",
"# Welcome to Hone\n\
\n\
Hone is a lightweight, cross-platform code editor built with\n\
TypeScript and compiled to native UI via the **Perry** compiler.\n\
\n\
## Features\n\
\n\
- Native performance on macOS, iOS, Windows, Linux, and Web\n\
- Built-in AI assistant with Claude integration\n\
- Git integration with staging, commits, and diffs\n\
- Full-text search with regex support\n\
- Syntax highlighting for 10+ languages\n\
- Extensible theme system\n\
\n\
## Getting Started\n\
\n\
Open a folder using **File > Open Folder** or click the folder\n\
icon in the activity bar to browse your local files.\n\
\n\
```typescript\n\
import { App, VStack, Text, Button } from 'perry/ui';\n\
\n\
App('Hello Hone', () => {\n\
  return VStack(8, [\n\
    Text('Welcome!'),\n\
    Button('Click me', () => { console.log('Hello'); }),\n\
  ]);\n\
});\n\
```\n\
\n\
## Architecture\n\
\n\
| Layer | Technology |\n\
|-------|------------|\n\
| UI    | Perry (TS → Native) |\n\
| Core  | TypeScript (bun test) |\n\
| Editor| Custom text engine |\n\
| AI    | Claude API |\n\
");

    file("/documents/package.json",
'{\n\
  "name": "hone-demo",\n\
  "version": "0.1.0",\n\
  "description": "Demo project for Hone IDE",\n\
  "main": "src/app.ts",\n\
  "scripts": {\n\
    "build": "perry compile src/app.ts --output demo",\n\
    "typecheck": "tsc --noEmit",\n\
    "test": "bun test"\n\
  },\n\
  "dependencies": {},\n\
  "devDependencies": {\n\
    "typescript": "^5.4.0"\n\
  }\n\
}\n');

    file("/documents/tsconfig.json",
'{\n\
  "compilerOptions": {\n\
    "target": "ES2023",\n\
    "module": "ESNext",\n\
    "moduleResolution": "bundler",\n\
    "strict": true,\n\
    "outDir": "dist",\n\
    "rootDir": "src"\n\
  },\n\
  "include": ["src/**/*.ts"]\n\
}\n');

    file("/documents/src/app.ts",
"import { App, VStack, HStack, Text, Button, Spacer } from 'perry/ui';\n\
import { greet } from './utils/greeting';\n\
import { Counter } from './components/counter';\n\
\n\
App('Hone Demo', () => {\n\
  const header = Text(greet('World'));\n\
  const counter = Counter();\n\
\n\
  return VStack(12, [\n\
    header,\n\
    counter,\n\
    Spacer(),\n\
    Text('Built with Perry'),\n\
  ]);\n\
});\n");

    file("/documents/src/components/counter.ts",
"import { VStack, HStack, Text, Button, textSetString } from 'perry/ui';\n\
\n\
let count = 0;\n\
let label: unknown = null;\n\
\n\
function updateLabel(): void {\n\
  if (label) {\n\
    textSetString(label, 'Count: ' + String(count));\n\
  }\n\
}\n\
\n\
export function Counter(): unknown {\n\
  label = Text('Count: 0');\n\
\n\
  const decBtn = Button(' - ', () => {\n\
    count = count - 1;\n\
    updateLabel();\n\
  });\n\
\n\
  const incBtn = Button(' + ', () => {\n\
    count = count + 1;\n\
    updateLabel();\n\
  });\n\
\n\
  return VStack(8, [\n\
    label,\n\
    HStack(8, [decBtn, incBtn]),\n\
  ]);\n\
}\n");

    file("/documents/src/utils/greeting.ts",
"/**\n\
 * Returns a greeting message.\n\
 * @param name - The name to greet\n\
 */\n\
export function greet(name: string): string {\n\
  const hour = new Date().getHours();\n\
  let timeOfDay = 'day';\n\
  if (hour < 12) {\n\
    timeOfDay = 'morning';\n\
  } else if (hour < 17) {\n\
    timeOfDay = 'afternoon';\n\
  } else {\n\
    timeOfDay = 'evening';\n\
  }\n\
  return 'Good ' + timeOfDay + ', ' + name + '!';\n\
}\n\
\n\
export function formatDate(date: Date): string {\n\
  const y = date.getFullYear();\n\
  const m = String(date.getMonth() + 1).padStart(2, '0');\n\
  const d = String(date.getDate()).padStart(2, '0');\n\
  return y + '-' + m + '-' + d;\n\
}\n");

    file("/documents/tests/greeting.test.ts",
"import { describe, it, expect } from 'bun:test';\n\
import { greet, formatDate } from '../src/utils/greeting';\n\
\n\
describe('greet', () => {\n\
  it('should return a greeting with the name', () => {\n\
    const result = greet('Hone');\n\
    expect(result).toContain('Hone');\n\
    expect(result).toMatch(/Good (morning|afternoon|evening|day), Hone!/);\n\
  });\n\
});\n\
\n\
describe('formatDate', () => {\n\
  it('should format a date as YYYY-MM-DD', () => {\n\
    const date = new Date(2026, 2, 13); // March 13, 2026\n\
    expect(formatDate(date)).toBe('2026-03-13');\n\
  });\n\
\n\
  it('should pad single-digit months and days', () => {\n\
    const date = new Date(2026, 0, 5); // January 5, 2026\n\
    expect(formatDate(date)).toBe('2026-01-05');\n\
  });\n\
});\n");

    // Set children arrays for directories
    C["/documents"].children = ["README.md", "package.json", "tsconfig.json", "src", "tests"];
    C["/documents/src"].children = ["app.ts", "components", "utils"];
    C["/documents/src/components"].children = ["counter.ts"];
    C["/documents/src/utils"].children = ["greeting.ts"];
    C["/documents/tests"].children = ["greeting.test.ts"];

})();

})();

// ── perry/system stubs added with the recent perry-ext landings ────
// These mirror the recently-merged FEATURES matrix entries:
//   #917 (share sheet), #675 (App Group), #976 (getOSVersion).
// Web has no native share/group equivalents; stubs are no-ops to
// satisfy the matrix-drift gate. Real Web Share API integration
// (navigator.share) is a follow-up.

function perry_system_share_text(_text, _title) {}
function perry_system_share_url(_url, _title) {}
function perry_system_app_group_set(key, value) {
    localStorage.setItem("perry_app_group_" + key, value);
}
function perry_system_app_group_get(key) {
    return localStorage.getItem("perry_app_group_" + key) || "";
}
function perry_system_app_group_delete(key) {
    localStorage.removeItem("perry_app_group_" + key);
}
function perry_system_get_os_version() {
    return (typeof navigator !== "undefined" && navigator.userAgent) || "";
}
