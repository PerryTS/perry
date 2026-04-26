/// Regression tests for issue #195: method-chain modifier syntax on perry/ui widgets
/// must produce a compile error rather than silently dropping the modifier.
///
/// Supported form:  Text("hi", { font: "title" })
/// Rejected form:   Text("hi").font("title")   ← compile error

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<perry_hir::Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts")
                .map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// Direct chain `Text("hi").font("title")` must fail with a diagnostic naming the modifier.
#[test]
fn text_dot_font_is_rejected() {
    let result = lower_result(r#"
        import { Text } from "perry/ui";
        Text("hi").font("title");
    "#);
    let err = result.unwrap_err();
    assert!(
        err.contains("modifier 'font'"),
        "expected error mentioning modifier 'font', got: {err}"
    );
}

/// `.color(...)` on a widget constructor must also be rejected.
#[test]
fn text_dot_color_is_rejected() {
    let result = lower_result(r#"
        import { Text } from "perry/ui";
        Text("hi").color("red");
    "#);
    let err = result.unwrap_err();
    assert!(
        err.contains("modifier 'color'"),
        "expected error mentioning modifier 'color', got: {err}"
    );
}

/// Chained modifiers fail on the FIRST one in the chain.
#[test]
fn chained_modifiers_fail_on_first() {
    let result = lower_result(r#"
        import { Text } from "perry/ui";
        Text("hi").font("title").color("red");
    "#);
    let err = result.unwrap_err();
    // Should mention 'font' (the first modifier in the chain).
    assert!(
        err.contains("modifier 'font'"),
        "expected error mentioning first modifier 'font', got: {err}"
    );
}

/// Zero-arg modifier `.bold()` must also be rejected.
#[test]
fn text_dot_bold_is_rejected() {
    let result = lower_result(r#"
        import { Text } from "perry/ui";
        Text("hi").bold();
    "#);
    let err = result.unwrap_err();
    assert!(
        err.contains("modifier 'bold'"),
        "expected error mentioning modifier 'bold', got: {err}"
    );
}

/// VStack with chained modifier must also be rejected.
#[test]
fn vstack_dot_padding_is_rejected() {
    let result = lower_result(r#"
        import { VStack, Text } from "perry/ui";
        VStack([Text("hi")]).padding(16);
    "#);
    let err = result.unwrap_err();
    assert!(
        err.contains("modifier 'padding'"),
        "expected error mentioning modifier 'padding', got: {err}"
    );
}

/// Plain widget call with no modifiers must compile without error.
#[test]
fn plain_widget_call_is_accepted() {
    let result = lower_result(r#"
        import { Text } from "perry/ui";
        const handle = Text("hi");
    "#);
    result.expect("Text(\"hi\") with no modifier should compile without error");
}
