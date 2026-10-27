use super::*;
use crate::p2::bindings::cli::{
    environment, exit, stderr, stdin, stdout, terminal_stdin, terminal_stdout,
};
use futures_lite::future::block_on;
use std::time::{Duration, Instant};
use wasmtime_wasi_io::streams::StreamError;

#[test]
fn cli_capabilities_default_closed_and_do_not_inherit_environment() {
    let mut ctx = crate::WasiCtx::new(4).unwrap();
    let mut table = ResourceTable::new();
    let mut view = WasiCliCtxView {
        ctx: &mut ctx.cli,
        table: &mut table,
    };
    assert!(environment::Host::get_arguments(&mut view)
        .unwrap()
        .is_empty());
    assert!(environment::Host::get_environment(&mut view)
        .unwrap()
        .is_empty());
    assert_eq!(environment::Host::initial_cwd(&mut view).unwrap(), None);
    assert!(terminal_stdin::Host::get_terminal_stdin(&mut view)
        .unwrap()
        .is_none());
    assert!(terminal_stdout::Host::get_terminal_stdout(&mut view)
        .unwrap()
        .is_none());
    let input = stdin::Host::get_stdin(&mut view).unwrap();
    assert!(matches!(
        view.table.get_mut(&input).unwrap().read(1),
        Err(StreamError::Closed)
    ));
    for output in [
        stdout::Host::get_stdout(&mut view).unwrap(),
        stderr::Host::get_stderr(&mut view).unwrap(),
    ] {
        let stream = view.table.get_mut(&output).unwrap();
        stream
            .write(bytes::Bytes::from_static(b"discarded by explicit default"))
            .unwrap();
        stream.flush().unwrap();
    }
    view.ctx.arguments = vec!["guest".into(), "argument".into()];
    view.ctx.environment = vec![("KEY".into(), "value".into())];
    view.ctx.initial_cwd = Some("/sandbox".into());
    assert_eq!(
        environment::Host::get_arguments(&mut view).unwrap(),
        ["guest", "argument"]
    );
    assert_eq!(
        environment::Host::get_environment(&mut view).unwrap(),
        [("KEY".into(), "value".into())]
    );
    assert_eq!(
        environment::Host::initial_cwd(&mut view).unwrap(),
        Some("/sandbox".into())
    );
}

#[test]
fn cli_memory_views_share_progress_and_exit_is_a_typed_trap() {
    let mut ctx = crate::WasiCtx::new(4).unwrap();
    ctx.cli.stdin = Box::new(MemoryInputPipe::new("abcdef"));
    let output = MemoryOutputPipe::new(16);
    ctx.cli.stdout = Box::new(output.clone());
    let mut table = ResourceTable::new();
    let mut view = WasiCliCtxView {
        ctx: &mut ctx.cli,
        table: &mut table,
    };
    let a = stdin::Host::get_stdin(&mut view).unwrap();
    let b = stdin::Host::get_stdin(&mut view).unwrap();
    assert_eq!(
        view.table.get_mut(&a).unwrap().read(2).unwrap().as_ref(),
        b"ab"
    );
    assert_eq!(
        view.table.get_mut(&b).unwrap().read(2).unwrap().as_ref(),
        b"cd"
    );
    let w = stdout::Host::get_stdout(&mut view).unwrap();
    view.table
        .get_mut(&w)
        .unwrap()
        .write(bytes::Bytes::from_static(b"captured"))
        .unwrap();
    assert_eq!(output.contents().as_ref(), b"captured");
    for (status, expected) in [(Ok(()), 0), (Err(()), 1)] {
        let e = exit::Host::exit(&mut view, status).unwrap_err();
        assert_eq!(e.downcast_ref::<crate::I32Exit>().unwrap().0, expected);
    }
    assert_eq!(
        exit::Host::exit_with_code(&mut view, 201)
            .unwrap_err()
            .downcast_ref::<crate::I32Exit>()
            .unwrap()
            .0,
        201
    );
    // Ordinary resource drop releases only that view.
    view.table.delete(a).unwrap();
    assert_eq!(
        view.table.get_mut(&b).unwrap().read(2).unwrap().as_ref(),
        b"ef"
    );
}

fn wait_empty(driver: &perry_component_io::Driver) {
    let until = Instant::now() + Duration::from_secs(5);
    while driver.outstanding() != 0 {
        assert!(
            Instant::now() < until,
            "stdio cancellation did not complete"
        );
        std::thread::yield_now();
    }
}

#[test]
fn native_stdio_shares_cursor_cancels_last_view_and_preserves_process_handles() {
    const FLAG: &str = "PERRY_WASI_STDIO_CHILD";
    if std::env::var_os(FLAG).is_some() {
        let driver = perry_component_io::Driver::new(4).unwrap();
        let input = NativeInput::stdin(driver.submitter());
        let mut a = input.p2_stream();
        let mut b = input.p2_stream();
        assert!(a.read(3).unwrap().is_empty());
        drop(a);
        assert_eq!(driver.outstanding(), 1);
        let timer = driver
            .submit(|io| async move { io.sleep(Duration::from_millis(5)).await })
            .unwrap();
        block_on(timer).unwrap().unwrap();
        eprintln!("PERRY_STDIO_READY");
        block_on(b.ready());
        assert_eq!(b.read(2).unwrap().as_ref(), b"ab");
        let mut c = input.p2_stream();
        assert_eq!(c.read(9).unwrap().as_ref(), b"c");
        assert_eq!(block_on(c.blocking_read(3)).unwrap().as_ref(), b"def");
        assert!(b.read(1).unwrap().is_empty());
        block_on(b.cancel());
        assert_eq!(driver.outstanding(), 1);
        block_on(c.cancel());
        assert_eq!(
            driver.outstanding(),
            0,
            "cancel must await driver acknowledgement"
        );
        let output = NativeOutput::stdout(driver.submitter());
        let mut x = output.p2_stream();
        let mut y = output.p2_stream();
        assert!(x.check_write().unwrap() >= 4);
        x.write(bytes::Bytes::from(vec![b'x'; crate::MAX_READ_SIZE_ALLOC]))
            .unwrap();
        assert_eq!(x.check_write().unwrap(), 0);
        eprintln!("PERRY_STDIO_OUTPUT_PENDING");
        block_on(x.ready());
        x.write(bytes::Bytes::from_static(b"one|")).unwrap();
        block_on(x.ready());
        x.flush().unwrap();
        block_on(y.blocking_write_and_flush(bytes::Bytes::from_static(b"two|"))).unwrap();
        block_on(x.cancel());
        block_on(y.cancel());
        std::io::Write::write_all(&mut std::io::stdout(), b"original-alive|").unwrap();
        let errors = NativeOutput::stderr(driver.submitter());
        block_on(
            errors
                .p2_stream()
                .blocking_write_and_flush(bytes::Bytes::from_static(b"native-stderr\n")),
        )
        .unwrap();
        wait_empty(&driver);
        drop(driver);
        let mut stopped = input.p2_stream();
        let _ = stopped.read(1);
        assert!(matches!(stopped.read(1), Err(StreamError::Trap(_))));
        return;
    }
    use std::io::{BufRead, Read, Write};
    use std::process::{Command, Stdio};
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    // Keep the explicit pipes in the child under panic=abort too; the normal
    // libtest abort wrapper substitutes stdin before invoking a test.
    let mut child=ChildGuard(Command::new(std::env::current_exe().unwrap())
        .args(["--exact","cli::tests::native_stdio_shares_cursor_cancels_last_view_and_preserves_process_handles","--nocapture","--test-threads=1"])
        .args(["-Z", "unstable-options", "--force-run-in-process"])
        .env(FLAG,"1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap());
    let mut input = child.0.stdin.take().unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let stderr = child.0.stderr.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let errors = std::thread::spawn(move || {
        let mut collected = String::new();
        for line in std::io::BufReader::new(stderr).lines() {
            let line = line.unwrap();
            eprintln!("stdio child: {line}");
            if line == "PERRY_STDIO_READY" || line == "PERRY_STDIO_OUTPUT_PENDING" {
                send.send(line.clone()).unwrap();
            }
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    });
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(10))
            .expect("child reached pending native stdin"),
        "PERRY_STDIO_READY"
    );
    input.write_all(b"abcdef").unwrap();
    input.flush().unwrap();
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(10))
            .expect("child reached output backpressure"),
        "PERRY_STDIO_OUTPUT_PENDING"
    );
    let output = std::thread::spawn(move || {
        let mut data = Vec::new();
        let mut stdout = stdout;
        stdout.read_to_end(&mut data).unwrap();
        data
    });
    // Keep stdin open: cancelling the final view must release a live read, not EOF.
    let until = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < until, "native stdio child hung");
        std::thread::sleep(Duration::from_millis(10));
    };
    let errors = errors.join().unwrap();
    let output = String::from_utf8(output.join().unwrap()).unwrap();
    assert!(status.success(), "{errors}\n{output}");
    assert!(
        output.contains(&"x".repeat(crate::MAX_READ_SIZE_ALLOC)),
        "missing complete bounded write"
    );
    assert!(
        output.contains("one|two|original-alive|"),
        "missing stdout or preserved process handle"
    );
    assert!(errors.contains("native-stderr"), "{errors}");
}

#[cfg(unix)]
#[test]
fn native_stdin_directory_error_is_typed_then_closed() {
    const FLAG: &str = "WASI_PROVIDER_DIRECTORY_CHILD";
    if std::env::var_os(FLAG).is_some() {
        let driver = perry_component_io::Driver::new(4).unwrap();
        let input = NativeInput::stdin(driver.submitter());
        let mut view = input.p2_stream();
        let error = block_on(view.blocking_read(1)).unwrap_err();
        match error {
            StreamError::LastOperationFailed(e) => {
                let native = e.downcast_ref::<std::io::Error>().unwrap();
                // epoll rejects registering directories with EPERM; other
                // backends can accept the handle and reject read with EISDIR.
                assert!(native.raw_os_error().is_some());
                assert!(matches!(
                    native.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::IsADirectory
                ));
            }
            other => panic!("expected typed directory error, got {other:?}"),
        }
        assert!(matches!(view.read(1), Err(StreamError::Closed)));
        wait_empty(&driver);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let file = std::fs::File::open(directory.path()).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cli::tests::native_stdin_directory_error_is_typed_then_closed",
            "--nocapture",
            "--test-threads=1",
            "-Z",
            "unstable-options",
            "--force-run-in-process",
        ])
        .env(FLAG, "1")
        .stdin(std::process::Stdio::from(file))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn component_exit_preserves_guest_status_as_host_trap() {
    struct State {
        ctx: crate::WasiCtx,
        table: ResourceTable,
    }
    impl crate::WasiView for State {
        fn ctx(&mut self) -> crate::WasiCtxView<'_> {
            crate::WasiCtxView {
                ctx: &mut self.ctx,
                table: &mut self.table,
            }
        }
    }
    for asynchronous in [false, true] {
        let engine = wasmtime::Engine::default();
        let component = wasmtime::component::Component::new(
            &engine,
            r#"(component
          (import "wasi:cli/exit@0.2.0" (instance $exit
            (export "exit" (func (param "status" (result))))))
          (alias export $exit "exit" (func $exit))
          (core func $lower (canon lower (func $exit)))
          (core module $guest
            (import "env" "exit" (func $exit (param i32)))
            (func (export "leave") (param i32) (call $exit (local.get 0))))
          (core instance $env (export "exit" (func $lower)))
          (core instance $guest (instantiate $guest (with "env" (instance $env))))
          (alias core export $guest "leave" (core func $leave))
          (func (export "leave") (param "status" (result)) (canon lift (core func $leave))))"#,
        )
        .unwrap();
        let mut linker = wasmtime::component::Linker::new(&engine);
        if asynchronous {
            crate::p2::add_to_linker_async(&mut linker).unwrap();
        } else {
            crate::p2::add_to_linker_sync(&mut linker).unwrap();
        }
        for (status, expected) in [(Ok(()), 0), (Err(()), 1)] {
            let mut store = wasmtime::Store::new(
                &engine,
                State {
                    ctx: crate::WasiCtx::new(4).unwrap(),
                    table: ResourceTable::new(),
                },
            );
            let instance = if asynchronous {
                block_on(linker.instantiate_async(&mut store, &component)).unwrap()
            } else {
                linker.instantiate(&mut store, &component).unwrap()
            };
            let leave = instance
                .get_typed_func::<(Result<(), ()>,), ()>(&mut store, "leave")
                .unwrap();
            let err = if asynchronous {
                block_on(leave.call_async(&mut store, (status,))).unwrap_err()
            } else {
                leave.call(&mut store, (status,)).unwrap_err()
            };
            assert_eq!(err.downcast_ref::<crate::I32Exit>().unwrap().0, expected);
        }
    }
}
