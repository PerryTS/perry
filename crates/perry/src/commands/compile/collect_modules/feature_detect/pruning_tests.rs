use super::tests::detect_for_source;

#[test]
fn unrelated_class_methods_do_not_select_regex() {
    for method in ["search", "match", "matchAll"] {
        for source in [
            format!("class Probe {{ {method}(): number {{ return 1; }} }} console.log(new Probe().{method}());"),
            format!("class Probe {{ {method}(): number {{ return 1; }} }} function f() {{ return new Probe().{method}(); }} console.log(f());"),
            format!("class Probe {{ {method}(): number {{ return 1; }} }} class C {{ f() {{ return new Probe().{method}(); }} }} console.log(new C().f());"),
            format!("class Probe {{ {method}(): number {{ return 1; }} }} const p = new Probe(); console.log(p.{method}());"),
            format!("class Probe {{ constructor() {{}} {method}(): number {{ return 1; }} }} console.log(new Probe().{method}());"),
            format!("class Probe {{ constructor() {{ return this; }} {method}(): number {{ return 1; }} }} console.log(new Probe().{method}());"),
            format!("class Probe {{ {method}(): number {{ return 1; }} }} const f = () => new Probe().{method}(); console.log(f());"),
        ] {
            assert!(!detect_for_source(&source).uses_regex, "{source}");
        }
    }
}

#[test]
fn regex_operations_and_unproven_receivers_keep_the_engine() {
    for source in [
        r#"console.log('abc'.search('b'));"#,
        r#"console.log('abc'.match('b'));"#,
        r#"console.log([... 'abc'.matchAll('b')].length);"#,
        r#"console.log(/b/.test('abc'));"#,
        r#"console.log(new RegExp('b').exec('abc'));"#,
        r#"function f(x: any) { return x.search('b'); } f('abc');"#,
        r#"function f(x: any) { return x.match('b'); } f('abc');"#,
        r#"function f(x: any) { return x.matchAll('b'); } f('abc');"#,
        r#"class Probe { search() { return 'abc'.search('b'); } } new Probe().search();"#,
        r#"class Probe { match() { return /b/.test('abc'); } } new Probe().match();"#,
        r#"class Probe { search() { return 1; } } function f(x: Probe) { return x.search(); }"#,
        r#"class Probe { constructor() { return value; } search() { return 1; } } new Probe().search();"#,
        r#"class Probe { constructor() { if (flag) { return value; } } search() { return 1; } } new Probe().search();"#,
        r#"class Probe { search() { return 1; } } let p: any = new Probe(); p = value; p.search('b');"#,
        r#"class Probe { search() { return 1; } } const p = value as Probe; p.search();"#,
        r#"class Probe { constructor() { Object.setPrototypeOf(this, String.prototype); } search() { return 1; } } new Probe().search();"#,
        r#"class Probe { search() { return 1; } } const p = new Probe(); mutate(p); p.search();"#,
        r#"class Probe { search() { return 1; } } mutate(Probe.prototype); new Probe().search();"#,
        r#"class Probe extends Base { search() { return 1; } } new Probe().search();"#,
        r#"class Probe { search() { return 1; } } new Probe('abc'.search('b')).search();"#,
        r#"class C { f(x = 'abc'.search('b')) { return x; } }"#,
        r#"const f = (x = 'abc'.search('b')) => x;"#,
    ] {
        assert!(detect_for_source(source).uses_regex, "{source}");
    }
}

#[test]
fn discarded_signal_arrow_listeners_do_not_select_ipc() {
    for method in [
        "on",
        "once",
        "addListener",
        "prependListener",
        "prependOnceListener",
    ] {
        for event in ["SIGINT", "SIGTERM", "beforeExit", "exit"] {
            let source = format!("process.{method}('{event}', () => {{}});");
            let ctx = detect_for_source(&source);
            assert!(!ctx.uses_proc_ipc, "{source}");
            assert!(!ctx.uses_regex, "{source}");
            assert!(ctx.uses_os, "signal hooks must stay available: {source}");
        }
    }
}

#[test]
fn assigned_signal_arrows_preserve_identity_without_selecting_ipc() {
    for method in [
        "on",
        "once",
        "addListener",
        "prependListener",
        "prependOnceListener",
    ] {
        for event in ["SIGINT", "SIGTERM", "beforeExit", "exit"] {
            for handler in ["callback = () => {}", "callback = other = () => {}"] {
                let source = format!(
                    "let callback: () => void; let other: () => void; \
                     process.{method}('{event}', {handler}); \
                     process.removeListener('{event}', callback);"
                );
                for source in [
                    source.clone(),
                    format!("function setup() {{ {source} }} setup();"),
                ] {
                    let ctx = detect_for_source(&source);
                    assert!(!ctx.uses_proc_ipc, "{source}");
                    assert!(!ctx.uses_regex, "{source}");
                    assert!(ctx.uses_os, "signal hooks must stay available: {source}");
                }
            }
        }
    }
}

#[test]
fn assigned_listeners_keep_unproven_callbacks_and_escapes() {
    for source in [
        r#"process.on('SIGINT', callback = function () {});"#,
        r#"process.on('SIGINT', callback = function () { return this; });"#,
        r#"process.on('SIGINT', callback = unknown);"#,
        r#"process.on('SIGINT', callback = makeCallback());"#,
        r#"process.on('SIGINT', callback = flag ? () => {} : unknown);"#,
        r#"process.on(event, callback = () => {});"#,
        r#"process.on('unknown-event', callback = () => {});"#,
        r#"process.on('message', callback = () => {});"#,
        r#"process.once('disconnect', callback = () => {});"#,
        r#"const p = process.on('SIGINT', callback = () => {}); console.log(p);"#,
        r#"process.on('SIGINT', callback = () => {}).on('message', () => {});"#,
        r#"consume(process.on('SIGINT', callback = () => {}));"#,
        r#"function f() { return process.once('SIGTERM', callback = () => {}); }"#,
        r#"process.on('SIGINT', holder.callback = () => {});"#,
        r#"process.on('SIGINT', holder[key] = () => {});"#,
        r#"process.on('SIGINT', callback = holder.callback = () => {});"#,
        r#"const holder = { set callback(value: any) { consume(value); } }; process.on('SIGINT', holder.callback = () => {});"#,
        r#"const p = process; p.on('SIGINT', callback = () => {});"#,
        r#"process.on('SIGINT', callback = () => { const p = process; consume(p); });"#,
        r#"process.on('SIGINT', callback = () => { process.on('message', () => {}); });"#,
        r#"process.on('SIGINT', callback = () => { process.send('hello'); });"#,
        r#"process.on('SIGINT', callback = (p = process) => consume(p));"#,
        r#"process.on('SIGINT', callback = () => process);"#,
        r#"process.on('SIGINT', callback = () => { consume(globalThis); });"#,
    ] {
        let source = format!("let callback: any; {source}");
        let ctx = detect_for_source(&source);
        assert!(ctx.uses_proc_ipc, "{source}");
    }
}

#[test]
fn assigned_signal_callback_bodies_and_defaults_still_select_regex() {
    for handler in [
        r#"() => { console.log('abc'.search('b')); }"#,
        r#"(value = 'abc'.search('b')) => console.log(value)"#,
    ] {
        let source = format!("let callback: any; process.on('SIGINT', callback = {handler});");
        let ctx = detect_for_source(&source);
        assert!(!ctx.uses_proc_ipc, "{source}");
        assert!(ctx.uses_regex, "{source}");
    }
}

#[test]
fn discarded_listener_context_reaches_nested_statements() {
    for source in [
        r#"function f() { process.on('SIGINT', () => {}); } f();"#,
        r#"const f = () => { process.once('SIGTERM', () => {}); }; f();"#,
        r#"class C { f() { process.on('exit', () => {}); } } new C().f();"#,
        r#"if (true) { process.on('SIGINT', () => {}); }"#,
        r#"try { process.on('SIGINT', () => {}); } finally { process.on('exit', () => {}); }"#,
        r#"for (let i = 0; i < 1; i++) { process.on('SIGTERM', () => {}); }"#,
        r#"switch (1) { case 1: process.on('SIGINT', () => {}); break; }"#,
    ] {
        assert!(!detect_for_source(source).uses_proc_ipc, "{source}");
    }
}

#[test]
fn ipc_events_callbacks_and_namespace_escapes_keep_support() {
    for source in [
        r#"process.on('message', () => {});"#,
        r#"process.once('disconnect', () => {});"#,
        r#"process.on(event, () => {});"#,
        r#"process.on('unknown-event', () => {});"#,
        r#"process.on('SIGINT', function () { return this; });"#,
        r#"process.on('SIGINT', function () {});"#,
        r#"const listener = () => {}; process.on('SIGINT', listener);"#,
        r#"const p = process.on('SIGINT', () => {}); console.log(p);"#,
        r#"process.on('SIGINT', () => {}).on('message', () => {});"#,
        r#"consume(process.on('SIGINT', () => {}));"#,
        r#"function f() { return process.on('SIGINT', () => {}); }"#,
        r#"const f = () => process.on('SIGINT', () => {});"#,
        r#"class C { p = process.on('SIGINT', () => {}); }"#,
        r#"function f(p = process.on('SIGINT', () => {})) {}"#,
        r#"const p = process; p.on('SIGINT', () => {});"#,
        r#"const on = process.on; on('SIGINT', () => {});"#,
        r#"const p = globalThis.process; p.on('SIGINT', () => {});"#,
        r#"const p = globalThis['process']; p.on('SIGINT', () => {});"#,
        r#"import p from 'node:process'; consume(p); p.on('SIGINT', () => {});"#,
        r#"import * as p from 'node:process'; consume(p);"#,
        r#"process.on('SIGINT', () => { const p = process; consume(p); });"#,
        r#"process.on('SIGINT', () => { process.on('message', () => {}); });"#,
        r#"process.on('SIGINT', (p = process) => consume(p));"#,
        r#"process.on('SIGINT', () => process);"#,
        r#"process.send({a: 1});"#,
        r#"console.log(process.connected, process.channel);"#,
        r#"process.disconnect();"#,
    ] {
        assert!(detect_for_source(source).uses_proc_ipc, "{source}");
    }
}

#[test]
fn signal_callback_code_is_still_scanned_for_regex() {
    let ctx =
        detect_for_source(r#"process.on("SIGINT", () => { console.log("abc".search("b")); });"#);
    assert!(!ctx.uses_proc_ipc);
    assert!(ctx.uses_regex);
}

#[test]
fn static_initializers_can_expose_the_constructor_through_this() {
    let ctx = detect_for_source(
        r#"
        let escaped: any;
        function keep(value: any) { escaped = value; }
        class Probe { static { keep(this); } search() { return 1; } }
        console.log(escaped.name, new Probe().search());
    "#,
    );
    assert!(ctx.uses_regex);
}
