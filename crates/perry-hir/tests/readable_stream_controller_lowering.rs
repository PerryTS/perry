//! RSTREAM: a stream's controller is its own value (stream id + controller
//! bit); its calls dispatch at run time on that handle, never on a stream. `s.enqueue` on a ReadableStream is not a member in Node.
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
fn controller_calls_dispatch_on_the_controller_handle() {
    let dbg = lowered_debug(
        r#"
        import { ReadableStream, TransformStream } from "node:stream/web";
        const s = new ReadableStream({ start(c) { c.enqueue(1); c.error(2); } });
        const t = new TransformStream({ transform(chunk, tc) { tc.enqueue(chunk); } });
        const u = new ReadableStream({ pull: (c) => { c.enqueue(3); } });
    "#,
    );
    // One path: every controller call dispatches at run time on the
    // controller handle (stream id + controller bit), whose kind knows the
    // family. Nothing lowers a controller member statically.
    assert!(!dbg.contains("readable_stream_controller"), "{dbg}");
    for m in ["enqueue", "error"] {
        assert!(!dbg.contains(&format!("method: \"{m}\"")), "{m}: {dbg}");
    }
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
