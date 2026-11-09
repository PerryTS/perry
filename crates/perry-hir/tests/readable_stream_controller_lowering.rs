//! RSTREAM: a stream's controller is its own value (stream id + controller
//! bit), so the static controller lowering keys on the controller parameter,
//! never on a stream. `s.enqueue` on a ReadableStream is not a member in Node.
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lowered_debug(src: &str) -> String {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "rs_controller.ts", &mut cache)
                .expect("parse should succeed");
            let module = lower_module(&parsed.module, "test", "rs_controller.ts")
                .expect("lowering should succeed");
            format!("{module:#?}")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn controller_parameter_lowers_on_the_controller_module() {
    let dbg = lowered_debug(
        r#"
        import { ReadableStream, TransformStream } from "node:stream/web";
        const s = new ReadableStream({ start(c) { c.enqueue(1); c.error(2); } });
        const t = new TransformStream({ transform(chunk, tc) { tc.enqueue(chunk); } });
        const u = new ReadableStream({ pull: (c) => { c.enqueue(3); } });
    "#,
    );
    // Each controller call that lowers statically names the controller
    // module (one module string per NativeMethodCall); the others go through
    // the controller handle's own dispatch. None lowers on the stream module.
    let statics =
        dbg.matches("method: \"enqueue\"").count() + dbg.matches("method: \"error\"").count();
    assert_eq!(
        dbg.matches("\"readable_stream_controller\"").count(),
        statics,
        "{dbg}"
    );
}

#[test]
fn stream_never_reaches_the_controller_lowering() {
    let dbg = lowered_debug(
        r#"
        import { ReadableStream } from "node:stream/web";
        const s = new ReadableStream({});
        s.enqueue(1);
        s.close();
        const d = s.desiredSize;
    "#,
    );
    assert!(!dbg.contains("readable_stream_controller"), "{dbg}");
    for m in ["enqueue", "close", "desiredSize"] {
        assert!(!dbg.contains(&format!("method: \"{m}\"")), "{m}: {dbg}");
    }
}
