// Regression for #442: Windows inline `Button(label, onPress, { style })`
// didn't paint backgroundColor (explicit `widgetSetBackgroundColor` did).
//
// Root cause: `apply_inline_style` in
// `crates/perry-codegen/src/lower_call/ui_styling.rs` declared every
// styling FFI as DOUBLE-returning + emitted `block().call(DOUBLE, …)`,
// but the runtime functions in `perry-ui-{macos,windows,…}/src/lib.rs`
// are all `extern "C" fn(handle: i64, …)` — i.e. void return. The IR
// type mismatch (call double @void_fn(...)) was silently accepted by
// LLVM but produced inconsistent IR. The dispatch-table path for the
// equivalent explicit setter (`widgetSetBackgroundColor(btn, r, g, b, a)`)
// went through `lower_perry_ui_table_call` which correctly used VOID +
// `call_void` for `UiReturnKind::Void` rows, so the explicit form
// emitted clean IR while the inline form emitted bad IR.
//
// Fix: switch every `apply_inline_style` callsite to push VOID +
// `call_void`, matching the runtime ABI byte-for-byte (13 callsites:
// borderRadius / opacity / borderWidth / tooltip / hidden / enabled /
// backgroundColor / color / borderColor / padding / shadow /
// textDecoration / gradient).
//
// This program just needs to *compile* cleanly with the inline-style
// form on macOS — Windows is the bug-trigger platform but compile-smoke
// only runs on Linux/macOS without a windowing toolkit installed.
// Visual verification on the user's Windows host is the next step.

import { App, Button, VStack, widgetSetBackgroundColor } from 'perry/ui';

function main(): void {
    // Form A — explicit setter — works on Windows pre-fix.
    const widgetStyleButton = Button("widget style button", () => console.log('click A'));
    widgetSetBackgroundColor(widgetStyleButton, 9 / 255, 241 / 255, 180 / 255, 0.8);

    // Form B — inline style arg — broken on Windows pre-fix.
    const inlineStyleButton = Button("inline style button", () => console.log("click B"), {
        backgroundColor: { r: 245 / 255, g: 0 / 255, b: 45 / 255, a: 0.8 },
    });

    App({ title: "Layout Demo", width: 400, height: 300, body: VStack(16, [
        widgetStyleButton, inlineStyleButton,
    ])});
}
main();
