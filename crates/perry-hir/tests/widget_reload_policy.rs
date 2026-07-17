/// End-to-end lowering tests for the widget `reloadPolicy` field: a literal
/// `reloadPolicy: { after: { minutes: N } }` in the provider's return value
/// must land in `WidgetDecl::reload_after_seconds` (in seconds), and a
/// widget without one must leave the field `None` so each backend applies
/// its platform default.
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> perry_hir::Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn provider_reload_policy_lands_in_widget_decl() {
    let module = lower(
        r#"
        import { Widget, VStack, Text } from "perry/widget";

        const w = Widget({
            kind: "com.test.Weather",
            displayName: "Weather",
            description: "Current conditions",
            entryFields: { temperature: "number" },
            provider: async () => {
                return {
                    entries: [{ temperature: 21 }],
                    reloadPolicy: { after: { minutes: 45 } },
                };
            },
            render: (entry) => VStack([Text("hi")]),
        });
    "#,
    );
    assert_eq!(module.widgets.len(), 1, "expected one widget decl");
    assert_eq!(
        module.widgets[0].reload_after_seconds,
        Some(2700),
        "45 minutes must lower to 2700 seconds"
    );
}

#[test]
fn provider_without_reload_policy_leaves_none() {
    let module = lower(
        r#"
        import { Widget, VStack, Text } from "perry/widget";

        const w = Widget({
            kind: "com.test.Plain",
            displayName: "Plain",
            description: "No policy",
            entryFields: { temperature: "number" },
            provider: async () => {
                return { entries: [{ temperature: 21 }] };
            },
            render: (entry) => VStack([Text("hi")]),
        });
    "#,
    );
    assert_eq!(module.widgets.len(), 1, "expected one widget decl");
    assert_eq!(
        module.widgets[0].reload_after_seconds, None,
        "no reloadPolicy must leave reload_after_seconds unset"
    );
}

#[test]
fn multiple_distinct_policies_use_smallest() {
    // Error-retry shape from the docs: 5 minutes on the failure path,
    // 15 minutes on the happy path — the smallest literal wins because
    // the refresh interval is a single compile-time constant.
    let module = lower(
        r#"
        import { Widget, VStack, Text } from "perry/widget";

        const w = Widget({
            kind: "com.test.Retry",
            displayName: "Retry",
            description: "Retry policy",
            entryFields: { temperature: "number" },
            provider: async () => {
                const ok = false;
                if (!ok) {
                    return {
                        entries: [{ temperature: 0 }],
                        reloadPolicy: { after: { minutes: 5 } },
                    };
                }
                return {
                    entries: [{ temperature: 21 }],
                    reloadPolicy: { after: { minutes: 15 } },
                };
            },
            render: (entry) => VStack([Text("hi")]),
        });
    "#,
    );
    assert_eq!(module.widgets.len(), 1, "expected one widget decl");
    assert_eq!(
        module.widgets[0].reload_after_seconds,
        Some(300),
        "smallest literal policy (5 minutes) must win"
    );
}

#[test]
fn literal_plus_non_literal_policy_falls_back_to_platform_default() {
    // A provider mixing a readable literal with a policy the compiler
    // can't evaluate: the literal must NOT be adopted for the whole
    // widget. Honoring it would silently apply the error path's interval
    // to the branch whose policy is unknown — and would contradict the
    // warning, which says the platform default applies. `None` is what
    // makes every backend fall back to its own default.
    let module = lower(
        r#"
        import { Widget, VStack, Text } from "perry/widget";

        const dynamicPolicy = { after: { minutes: 5 } };

        const w = Widget({
            kind: "com.test.Mixed",
            displayName: "Mixed",
            description: "One literal policy, one dynamic",
            entryFields: { temperature: "number" },
            provider: async (ctx) => {
                if (ctx.failed) {
                    return {
                        entries: [{ temperature: 0 }],
                        reloadPolicy: dynamicPolicy,
                    };
                }
                return {
                    entries: [{ temperature: 21 }],
                    reloadPolicy: { after: { minutes: 45 } },
                };
            },
            render: (entry) => VStack([Text("hi")]),
        });
    "#,
    );
    assert_eq!(module.widgets.len(), 1, "expected one widget decl");
    assert_eq!(
        module.widgets[0].reload_after_seconds, None,
        "an unreadable reloadPolicy must fall back to the platform default, \
not silently adopt the other return path's literal"
    );
}
