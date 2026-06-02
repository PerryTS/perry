// Issue #699 — behavioral coverage for the runtime UI FFI surface.
//
// This file consolidates host-independent TUI smoke checks into a
// single fixture so the per-FFI symbol inventory in
// `test_ffi_surface_runtime_ui.ts` can drop the TUI/render/hook/widget
// entries listed in the `@covers` block at the bottom. The file is
// `node --experimental-strip-types`-incompatible (perry/tui has no
// Node equivalent) so the parity runner skips it; the compile-smoke
// job validates that every TUI call site still lowers cleanly.
//
// The tests are deliberately deterministic: no `run()` loop, no
// interactive input. We invoke widget constructors, style setters,
// hook factories, state/ref/stdout handles, and one-shot render so
// each FFI in the @covers block actually executes at least once.
// Behavior assertions land on log lines a future regression can grep.

import {
  AnimatedSpinner,
  Box,
  Input,
  List,
  ProgressBar,
  Select,
  Spacer,
  Spinner,
  Table,
  Tabs,
  Text,
  TextArea,
  enter,
  exit,
  render,
  state,
  useApp,
  useEffect,
  useFocus,
  useFocusManager,
  useInput,
  useMemo,
  useRef,
  useState,
  useStateSet,
  useStateTuple,
  useStdout,
} from "perry/tui";

// ── widget node creation ───────────────────────────────────────────
const plain = Text("plain");
const styled = Text("styled", {
  fg: "red",
  bg: "#202020",
  bold: true,
  italic: true,
  underline: true,
  reverse: true,
});
render(Box([plain, styled]));
console.log("text_constructed=1");

// ── Box variants: bare, children-only, style-only, style+children ──
const empty = Box();
const childrenOnly = Box([Text("a"), Text("b")]);
const styleOnly = Box({ flexDirection: "row", gap: 1 });
const styled2 = Box(
  {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
    gap: 2,
    padding: { top: 1, right: 2, bottom: 1, left: 2 },
    width: "50%",
    height: 10,
    flexGrow: 1,
    flexShrink: 1,
    flexBasis: "30%",
  },
  [Text("left"), Spacer(), Text("right")],
);
render(empty);
render(childrenOnly);
render(styleOnly);
render(styled2);
console.log("box_variants_rendered=1");

// Percentage units on a separate box so we exercise both
// height_pct and the basis_pct setter from the Phase 3.5 surface.
const percentBox = Box({ width: "100%", height: "50%", flexBasis: "20%" }, [
  Text("pct"),
]);
render(percentBox);
console.log("percent_box_ok=1");

// Uniform padding still routes to set_padding (not set_padding_each).
const uniform = Box({ padding: 2 }, [Text("p")]);
render(uniform);
console.log("uniform_pad_ok=1");

// ── widgets: spacer / progress / spinners / input / list / select ──
render(
  Box([
    Text("widgets:"),
    Spacer(),
    ProgressBar(3, 10, 20),
    Spinner(0),
    Spinner(1),
    Spinner(2),
    Spinner(3),
    AnimatedSpinner(),
    AnimatedSpinner({ interval: 80, frames: ["◐", "◓", "◑", "◒"] }),
  ]),
);
console.log("widgets_rendered=1");

// Input has a 1-arg and a 2-arg form; the 2-arg form lowers via the
// dedicated `input_at` FFI when the cursor is supplied.
render(Box([Input("alice"), Input("bob", 1)]));
console.log("input_variants_ok=1");

// List with no selection (default -1) vs explicit Select.
render(Box([List(["one", "two", "three"]), Select(["one", "two", "three"], 1)]));
console.log("list_select_ok=1");

// TextArea — splits on \n into one Text per line.
render(TextArea("first\nsecond\nthird"));
console.log("textarea_ok=1");

// Table + Tabs — exercise the grid + tabbed-body shapes.
render(
  Table({
    headers: ["name", "age"],
    rows: [
      ["alice", "30"],
      ["bob", "25"],
    ],
    selected: 0,
  }),
);
console.log("table_ok=1");

render(
  Tabs({
    tabs: ["one", "two"],
    active: 0,
    body: [Text("body one"), Text("body two")],
  }),
);
console.log("tabs_ok=1");

// Explicit enter() before a render — paint the alt-screen header once.
enter();
render(Box([Text("after enter()")]));
console.log("enter_then_render_ok=1");

// ── reactive state: factory + getter/setter receivers ───────────────
const slot = state(0);
slot.set(42);
console.log("state_factory_get=" + slot.get());

// ── hooks: linear invocation outside run() — exercises FFI shape ────
const s0 = useState(7);
console.log("useState_first=" + s0);
useStateSet(0, 9);
const [t0, t1] = useStateTuple(11);
console.log("useStateTuple=" + t0 + "," + ((t1 as unknown as number) > 0 ? 1 : 0));

const ref = useRef(100);
console.log("useRef_initial=" + ref.get());
ref.set(123);
console.log("useRef_updated=" + ref.get());

const memo = useMemo(() => 5 * 6, []);
console.log("useMemo=" + memo);

let effectRan = 0;
useEffect(() => {
  effectRan = 1;
}, []);
console.log("useEffect_ran=" + effectRan);

// useApp / useStdout — singleton accessors. We exercise stdout's
// columns/rows/write but only test for non-negativity (terminal size
// is environment dependent).
const app = useApp();
console.log("useApp_ok=" + (app !== undefined ? 1 : 0));
const stdout = useStdout();
const cols = stdout.columns();
const rows = stdout.rows();
console.log("stdout_dims_ok=" + (cols >= 0 && rows >= 0 ? 1 : 0));
stdout.write("stdout_write_ok\n");

// useFocus / useInput — both touch the focus FFI even when called
// outside the run() loop. useFocus returns 0/1 indicating "is this
// the active focus target." Outside run(), value is 0.
const focused = useFocus(1, 1);
console.log("useFocus_outside_run=" + focused);
useInput((s: string) => {
  if (s === "q") exit();
});
console.log("useInput_registered=1");

// useFocusManager — returns the focus manager singleton. Methods
// (.focus / .focusNext / .focusPrevious) dispatch to the
// js_perry_tui_focus_manager_* FFIs. Outside run() these are
// no-ops, but the call sites still lower.
const fm = useFocusManager();
fm.focusNext();
fm.focusPrevious();
fm.focus(0);
console.log("useFocusManager_ok=1");

console.log("DONE");

/*
@covers
crates/perry-runtime/src/tui/ffi.rs:
  - js_perry_tui_animated_spinner
  - js_perry_tui_box_add_children_array
  - js_perry_tui_box_set_align_items
  - js_perry_tui_box_set_flex_basis
  - js_perry_tui_box_set_flex_basis_pct
  - js_perry_tui_box_set_flex_direction
  - js_perry_tui_box_set_flex_grow
  - js_perry_tui_box_set_flex_shrink
  - js_perry_tui_box_set_gap
  - js_perry_tui_box_set_height
  - js_perry_tui_box_set_height_pct
  - js_perry_tui_box_set_justify_content
  - js_perry_tui_box_set_padding
  - js_perry_tui_box_set_padding_each
  - js_perry_tui_box_set_width
  - js_perry_tui_box_set_width_pct
  - js_perry_tui_enter
  - js_perry_tui_input
  - js_perry_tui_input_at
  - js_perry_tui_list
  - js_perry_tui_progress_bar
  - js_perry_tui_render
  - js_perry_tui_select
  - js_perry_tui_spacer
  - js_perry_tui_spinner
  - js_perry_tui_table
  - js_perry_tui_tabs
  - js_perry_tui_text_area
  - js_perry_tui_text_styled
crates/perry-runtime/src/tui/hooks.rs:
  - js_perry_tui_focus
  - js_perry_tui_focus_manager_focus
  - js_perry_tui_focus_manager_focus_next
  - js_perry_tui_focus_manager_focus_previous
  - js_perry_tui_ref_get
  - js_perry_tui_ref_set
  - js_perry_tui_stdout_columns
  - js_perry_tui_stdout_rows
  - js_perry_tui_stdout_write
  - js_perry_tui_use_app
  - js_perry_tui_use_effect
  - js_perry_tui_use_focus
  - js_perry_tui_use_focus_manager
  - js_perry_tui_use_memo
  - js_perry_tui_use_ref
  - js_perry_tui_use_state
  - js_perry_tui_use_state_set
  - js_perry_tui_use_state_slot
  - js_perry_tui_use_state_tuple
  - js_perry_tui_use_stdout
  - perry_tui_state_setter_trampoline
crates/perry-runtime/src/tui/input.rs:
  - js_perry_tui_use_input
crates/perry-runtime/src/tui/state.rs:
  - js_perry_tui_state_alloc
  - js_perry_tui_state_get
  - js_perry_tui_state_set
*/
