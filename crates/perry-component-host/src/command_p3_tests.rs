use super::*;
use futures_lite::future::{block_on, poll_once};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn immediate(status: u8) -> String {
    format!(
        r#"(component
      (core func $return (canon task.return (result (result))))
      (core module $code
        (import "host" "return" (func $return (param i32)))
        (func (export "run") (result i32) (call $return (i32.const {status})) (i32.const 0))
        (func (export "callback") (param i32 i32 i32) (result i32) unreachable))
      (core instance $code (instantiate $code (with "host" (instance (export "return" (func $return))))))
      (func $run async (result (result))
        (canon lift (core func $code "run") async (callback (core func $code "callback"))))
      (instance $run (export "run" (func $run)))
      (export "wasi:cli/run@0.3.0" (instance $run)))"#
    )
}
fn exit(code: u8, start: bool, export: bool) -> String {
    super::tests::exit_source(code, true, start, export)
        .replace("@0.2.12", "@0.3.0").replace("@0.2.0", "@0.3.0")
        .replace("(func $run (result (result))", "(func $run async (result (result))")
        .replace("(func (export \"run\")", "(func (export \"callback\") (param i32 i32 i32) (result i32) unreachable)\n (func (export \"run\")")
        .replace("(canon lift (core func $code \"run\"))", "(canon lift (core func $code \"run\") async (callback (core func $code \"callback\")))")
}

#[test]
fn p3_commands_preserve_results_exits_and_host_survival() {
    for code in [0, 1] {
        assert_eq!(
            run_command(
                &wat::parse_str(immediate(code)).unwrap(),
                &Options::default()
            )
            .unwrap(),
            i32::from(code)
        );
    }
    for code in [0, 23, 255] {
        for start in [false, true] {
            assert_eq!(
                run_command(
                    &wat::parse_str(exit(code, start, true)).unwrap(),
                    &Options::default()
                )
                .unwrap(),
                i32::from(code)
            );
        }
    }
    assert_eq!(
        run_command(&wat::parse_str(immediate(0)).unwrap(), &Options::default()).unwrap(),
        0
    );
}

#[test]
fn p3_command_validation_precedes_initializers_and_keeps_versions_explicit() {
    for source in [
        exit(0, true, false),
        exit(0, true, true).replace("(result (result))", "(result u32)"),
        super::tests::exit_source(0, true, true, true)
            .replace("@0.2.12", "@0.3.0")
            .replace("@0.2.0", "@0.3.0"),
        immediate(0).replace("@0.3.0", "@0.3.1"),
    ] {
        let error = run_command(&wat::parse_str(source).unwrap(), &Options::default()).unwrap_err();
        assert!(format!("{error:#}").contains("validating wasi:cli/run"));
    }
    let source = immediate(0)
        .trim_end()
        .strip_suffix(')')
        .unwrap()
        .to_owned()
        + r#"
      (core module $other (func (export "run") (result i32) i32.const 5))
      (core instance $other (instantiate $other))
      (func $other (result u32) (canon lift (core func $other "run")))
      (instance $other (export "run" (func $other)))
      (export "wasi:cli/run@0.3.1" (instance $other)))"#;
    assert_eq!(
        run_command(&wat::parse_str(source).unwrap(), &Options::default()).unwrap(),
        0,
        "the exact 0.3.0 export must win over a different compatible version"
    );
}

#[test]
fn p3_command_traps_and_limits_remain_embedding_failures() {
    for source in [
        immediate(2),
        immediate(0).replace("(call $return (i32.const 0))", "unreachable"),
    ] {
        assert!(run_command(&wat::parse_str(source).unwrap(), &Options::default()).is_err());
    }
    let bytes = wat::parse_str(immediate(0).replace(
        "(call $return (i32.const 0))",
        "(loop $forever (br $forever))",
    ))
    .unwrap();
    let error = run_command(
        &bytes,
        &Options {
            fuel: 100,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("fuel"));
    let bytes = wat::parse_str(immediate(0).replace(
        "(func (export \"run\")",
        "(memory 2) (func (export \"run\")",
    ))
    .unwrap();
    let error = run_command(
        &bytes,
        &Options {
            memory_bytes: 65536,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("memory"));
}

struct DropOutput(Arc<AtomicBool>);
impl Drop for DropOutput {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl perry_wasi::cli::IsTerminal for DropOutput {
    fn is_terminal(&self) -> bool {
        false
    }
}
impl perry_wasi::cli::StdoutStream for DropOutput {
    fn p2_stream(&self) -> perry_wasi::io::bindings::wasi::io::streams::OutputStream {
        Box::new(perry_wasi::pipe::MemoryOutputPipe::new(0))
    }
}

#[test]
fn real_p3_command_suspends_and_cancellation_releases_its_provider_context() {
    let source = include_str!("command/p3-clock.component.wat");
    assert_eq!(
        run_command(&wat::parse_str(source).unwrap(), &Options::default()).unwrap(),
        0
    );
    let bytes =
        wat::parse_str(source.replace("i64.const 1000000", "i64.const 60000000000")).unwrap();
    let mut prepared = Prepared::new(&bytes, &Options::default()).unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    prepared.store.get().data_mut().wasi.view().ctx.cli.stdout =
        Box::new(DropOutput(dropped.clone()));
    let mut call = Box::pin(prepared.run());
    assert!(
        block_on(poll_once(call.as_mut())).is_none(),
        "the canonical clock command must actually suspend"
    );
    assert!(!dropped.load(Ordering::SeqCst));
    drop(call);
    assert!(
        dropped.load(Ordering::SeqCst),
        "cancelling the command must release its provider context"
    );
    assert_eq!(
        run_command(&wat::parse_str(immediate(0)).unwrap(), &Options::default()).unwrap(),
        0
    );
}

#[test]
fn p3_stdio_command_waits_for_real_stream_and_completion_futures() {
    let bytes = wat::parse_str(include_str!("command/p3-stdio.component.wat")).unwrap();
    for (length, capacity, expected_status) in [
        (0, 0, 0),
        (65537, 65537, 0),
        (3 * 1024 * 1024 + 137, 3 * 1024 * 1024 + 137, 0),
        (65537, 3, 1),
    ] {
        let payload: Vec<u8> = (0..length).map(|n| (n * 17 + 31) as u8).collect();
        let output = perry_wasi::pipe::MemoryOutputPipe::new(capacity);
        let mut prepared = Prepared::new(&bytes, &Options::default()).unwrap();
        {
            let cli = &mut prepared.store.get().data_mut().wasi.view().ctx.cli;
            cli.stdin = Box::new(perry_wasi::pipe::MemoryInputPipe::new(payload.clone()));
            cli.stdout = Box::new(output.clone());
        }
        assert_eq!(block_on(prepared.run()).unwrap(), expected_status);
        assert_eq!(output.contents().as_ref(), &payload[..capacity]);
    }
}

#[test]
fn p3_command_waits_for_task_exit_and_reports_post_return_traps() {
    // task.return publishes the result before the task exits. A one-shot
    // command must keep driving a subsequently suspended task and surface its
    // eventual trap instead of treating the early result as process success.
    let source = include_str!("command/p3-clock.component.wat")
        .replace("(call $return (i32.const 0))", "")
        .replace(
            "(global.set $start (call $now))",
            "(call $return (i32.const 0)) (global.set $start (call $now))",
        )
        .replace(
            "(call $drop-set (global.get $set))",
            "(call $drop-set (global.get $set)) unreachable",
        );
    let error = run_command(&wat::parse_str(source).unwrap(), &Options::default()).unwrap_err();
    assert!(format!("{error:#}").contains("trapped"));
}
