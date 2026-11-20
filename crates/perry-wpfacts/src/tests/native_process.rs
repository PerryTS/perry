//! The `process` module value is the global process object (node:
//! `require("process") === globalThis.process`), and its members are what
//! the realm declares on it: a declared native function reads that function,
//! a key the realm declares as a host array of strings (`argv`, `execArgv`)
//! reads the one array the runtime made (or what the program stored there).
//! An undeclared key stays ⊤.
use super::*;

/// `process.execArgv` and the `process` module value are modelled values.
#[test]
fn process_module_value_and_exec_argv_are_modelled() {
    for module in ["require(\"process\")", "require(\"node:process\")", "pn"] {
        let pf = facts(&format!(
            r#"
            import * as pn from "node:process";
            function f(v: any) {{ return v.q; }}
            function g(v: any) {{ return v.r; }}
            console.log(f(process.execArgv), g({module}));
        "#
        ));
        assert!(!site(&pf, "q").top, "{module}\n{}", pf.dump_text());
        assert!(!site(&pf, "r").top, "{module}\n{}", pf.dump_text());
    }
}

/// The elements of `process.execArgv` are the strings the runtime put
/// there, as those of `process.argv` are.
#[test]
fn exec_argv_elements_are_strings() {
    for key in ["execArgv", "argv"] {
        let pf = facts(&format!(
            r#"
            const a = process.{key};
            const s = a[0];
            console.log(s.length);
        "#
        ));
        let s = site(&pf, "length");
        assert!(
            !s.top && s.receivers.is_empty(),
            "{key}\n{}",
            pf.dump_text()
        );
    }
}

/// Named control: a store through the module value is a store into the
/// global `process`: `process.argv` reads what was stored.
#[test]
fn store_through_module_value_reaches_the_global_process() {
    for key in ["argv", "execArgv"] {
        let pf = facts(&format!(
            r#"
            class P {{ x: number; constructor(x: number) {{ this.x = x; }} }}
            const p = new P(1);
            (require("process") as any).{key} = [p];
            const v: any = (process as any).{key}[0];
            function h(o: any) {{ return o.w; }}
            console.log(h(v));
        "#
        ));
        let w = site(&pf, "w");
        assert!(
            w.top || w.receivers.iter().any(|r| r.ends_with(":P")),
            "{key}\n{}",
            pf.dump_text()
        );
    }
}

/// Named control: a `process.nextTick` callback that writes a program
/// object stays modelled, whichever way the function is reached.
#[test]
fn next_tick_callback_writes_stay_modelled() {
    for call in [
        "process.nextTick((o: any) => { o.x = \"s\"; }, p);",
        "const nt = process.nextTick; nt((o: any) => { o.x = \"s\"; }, p);",
        "const nt = require(\"process\").nextTick; nt((o: any) => { o.x = \"s\"; }, p);",
        "require(\"node:process\").nextTick((o: any) => { o.x = \"s\"; }, p);",
    ] {
        let pf = facts(&format!(
            r#"
            class P {{ x: any; constructor(x: number) {{ this.x = x; }} }}
            const p = new P(1);
            {call}
            console.log(p.x);
        "#
        ));
        let x = field(tmpl(&pf, "P"), "x");
        assert!(!x.readonly, "{call}\n{}", pf.dump_text());
    }
}
