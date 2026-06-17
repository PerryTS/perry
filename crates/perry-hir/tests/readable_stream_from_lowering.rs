use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                parse_typescript_with_cache(&src, "readable_stream_from_lowering.ts", &mut cache)
                    .expect("parse should succeed");
            lower_module(&parsed.module, "test", "readable_stream_from_lowering.ts")
                .expect("lowering should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn readable_stream_from_static_factory_lowers_to_native_factory() {
    let module = lower(
        r#"
        import { ReadableStream } from "node:stream/web";
        const rs = (ReadableStream as any).from(["a"]);
    "#,
    );

    let init = module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name,
                init: Some(expr),
                ..
            } if name == "rs" => Some(expr),
            _ => None,
        })
        .expect("expected rs binding");

    match init {
        Expr::NativeMethodCall {
            module,
            class_name,
            object,
            method,
            args,
        } => {
            assert_eq!(module, "readable_stream");
            assert_eq!(class_name.as_deref(), Some("ReadableStream"));
            assert!(object.is_none());
            assert_eq!(method, "from");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected ReadableStream.from NativeMethodCall, got: {other:#?}"),
    }
}
