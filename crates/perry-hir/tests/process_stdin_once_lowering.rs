//! `process.stdin.once(event, handler)` must lower to `Expr::ProcessStdinOn`,
//! exactly like `on` / `addListener`.
//!
//! Only `on` and `addListener` were matched, so `once` fell through to the
//! generic member-call path and never reached `js_readline_stdin_on` — the
//! callback was never registered with the fd-0 reader and simply never fired.
//!
//! That is what broke `echo hi | claude -p "…"`. Claude Code's print-mode
//! stdin reader is:
//!
//! ```js
//! process.stdin.on("data", acc);
//! const timedOut = await race(process.stdin.once("end"), timeout(3000));
//! ```
//!
//! With `once` dropped, the `end` half of that race could never win. The race
//! always fell through to the timer — and because that timer is `unref`'d,
//! nothing kept the event loop alive, so the process exited 0 having printed
//! nothing at all (node prints the result).

use perry_diagnostics::SourceCache;
use perry_hir::{clear_current_module_source, lower_module};
use perry_parser::parse_typescript_with_cache;

fn lower_debug(src: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "/tmp/stdin_once_test.ts", &mut cache)
        .expect("parse failed");
    let hir = lower_module(&parsed.module, "test", "/tmp/stdin_once_test.ts").expect("lower failed");
    clear_current_module_source();
    format!("{:#?}", hir.init)
}

#[test]
fn process_stdin_once_lowers_like_on() {
    let ir = lower_debug(r#"process.stdin.once("end", () => {});"#);
    assert!(
        ir.contains("ProcessStdinOn"),
        "process.stdin.once must lower to ProcessStdinOn so the handler reaches \
         readline's stdin listener registry:\n{ir}"
    );
}

#[test]
fn process_stdin_on_and_add_listener_still_lower() {
    let ir = lower_debug(
        r#"
        process.stdin.on("data", () => {});
        process.stdin.addListener("end", () => {});
        process.stdin.once("error", () => {});
    "#,
    );
    assert_eq!(
        ir.matches("ProcessStdinOn").count(),
        3,
        "on / addListener / once must all lower to ProcessStdinOn:\n{ir}"
    );
}
