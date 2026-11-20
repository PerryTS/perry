use super::*;
use crate::p3::tests::{with_store, State};
use crate::pipe::{MemoryInputPipe, MemoryOutputPipe};
use futures_lite::future::block_on;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::component::{Accessor, FutureConsumer, Source};
use wasmtime::StoreContextMut;

type Outcome = Result<(), ErrorCode>;
struct Receive(Option<oneshot::Sender<Outcome>>);
impl<D> FutureConsumer<D> for Receive {
    type Item = Outcome;
    fn poll_consume(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<Outcome>,
        finish: bool,
    ) -> Poll<wasmtime::Result<()>> {
        let mut value = None;
        source.read(store, &mut value)?;
        if let Some(value) = value {
            let _ = self.0.take().unwrap().send(value);
        } else if !finish {
            return Poll::Ready(Err(wasmtime::format_err!(
                "result future delivered no value"
            )));
        }
        Poll::Ready(Ok(()))
    }
}
fn cli_accessor(accessor: &Accessor<State>) -> Accessor<State, WasiCli> {
    accessor.with_getter(|state| WasiCliCtxView {
        ctx: &mut state.ctx.as_mut().unwrap().cli,
        table: &mut state.table,
    })
}
fn receive(
    accessor: &Accessor<State>,
    future: FutureReader<Outcome>,
) -> oneshot::Receiver<Outcome> {
    let (send, receive) = oneshot::channel();
    accessor
        .with(|store| future.pipe(store, Receive(Some(send))))
        .unwrap();
    receive
}

#[test]
fn cli_streams_preserve_large_bodies_and_exact_output_capacity() {
    with_store(|mut store| {
        for length in [0, 7, 65_537, 3 * 1024 * 1024 + 137] {
            let expected: Vec<u8> = (0..length).map(|i| (i * 17 + 31) as u8).collect();
            let output = MemoryOutputPipe::new(length);
            {
                let cli = &mut store.data_mut().ctx.as_mut().unwrap().cli;
                cli.stdin = Box::new(MemoryInputPipe::new(expected.clone()));
                cli.stdout = Box::new(output.clone());
            }
            block_on(store.run_concurrent(async |accessor| {
                let cli = cli_accessor(accessor);
                let (data, input) = cli
                    .with(<WasiCli as stdin::HostWithStore<State>>::read_via_stream)
                    .unwrap();
                let written = cli
                    .with(|store| {
                        <WasiCli as stdout::HostWithStore<State>>::write_via_stream(store, data)
                    })
                    .unwrap();
                let input = receive(accessor, input);
                let written = receive(accessor, written);
                assert_eq!(input.await.unwrap(), Ok(()));
                assert_eq!(written.await.unwrap(), Ok(()));
            }))
            .unwrap();
            assert_eq!(output.contents().as_ref(), expected);
            store.assert_concurrent_state_empty();
        }
    });
}

#[test]
fn closed_defaults_and_dropped_input_complete_successfully() {
    with_store(|mut store| {
        block_on(store.run_concurrent(async |accessor| {
            let cli = cli_accessor(accessor);
            let (data, input) = cli
                .with(<WasiCli as stdin::HostWithStore<State>>::read_via_stream)
                .unwrap();
            let output = cli
                .with(|store| {
                    <WasiCli as stderr::HostWithStore<State>>::write_via_stream(store, data)
                })
                .unwrap();
            let input = receive(accessor, input);
            let output = receive(accessor, output);
            assert_eq!(input.await.unwrap(), Ok(()));
            assert_eq!(output.await.unwrap(), Ok(()));
            let (mut data, input) = cli
                .with(<WasiCli as stdin::HostWithStore<State>>::read_via_stream)
                .unwrap();
            accessor.with(|store| data.close(store)).unwrap();
            assert_eq!(receive(accessor, input).await.unwrap(), Ok(()));
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
    });
}

#[test]
fn a_closed_output_reports_pipe_error_through_the_result_future() {
    with_store(|mut store| {
        let output = MemoryOutputPipe::new(3);
        {
            let cli = &mut store.data_mut().ctx.as_mut().unwrap().cli;
            cli.stdin = Box::new(MemoryInputPipe::new(vec![1, 2, 3, 4, 5, 6, 7]));
            cli.stdout = Box::new(output.clone());
        }
        block_on(store.run_concurrent(async |accessor| {
            let cli = cli_accessor(accessor);
            let (data, input) = cli
                .with(<WasiCli as stdin::HostWithStore<State>>::read_via_stream)
                .unwrap();
            let output = cli
                .with(|store| {
                    <WasiCli as stdout::HostWithStore<State>>::write_via_stream(store, data)
                })
                .unwrap();
            let input = receive(accessor, input);
            let output = receive(accessor, output);
            assert_eq!(output.await.unwrap(), Err(ErrorCode::Pipe));
            assert_eq!(input.await.unwrap(), Ok(()));
        }))
        .unwrap();
        assert_eq!(output.contents().as_ref(), [1, 2, 3]);
        store.assert_concurrent_state_empty();
    });
}

struct DelayedFactory(std::sync::Mutex<Option<DelayedOutput>>);
impl crate::cli::IsTerminal for DelayedFactory {
    fn is_terminal(&self) -> bool {
        false
    }
}
impl crate::cli::StdoutStream for DelayedFactory {
    fn p2_stream(&self) -> Box<dyn wasmtime_wasi_io::streams::OutputStream> {
        Box::new(self.0.lock().unwrap().take().unwrap())
    }
}
struct DelayedOutput {
    buffer: MemoryOutputPipe,
    started: Option<oneshot::Sender<()>>,
    release: Option<oneshot::Receiver<()>>,
    pending: bool,
    fail: bool,
    failed: bool,
}
#[async_trait::async_trait]
impl wasmtime_wasi_io::poll::Pollable for DelayedOutput {
    async fn ready(&mut self) {
        if self.pending {
            self.release.take().unwrap().await.unwrap();
            self.pending = false;
            self.failed = self.fail;
        }
    }
}
#[async_trait::async_trait]
impl wasmtime_wasi_io::streams::OutputStream for DelayedOutput {
    fn check_write(&mut self) -> wasmtime_wasi_io::streams::StreamResult<usize> {
        if self.pending {
            Ok(0)
        } else {
            self.buffer.check_write()
        }
    }
    fn write(&mut self, bytes: bytes::Bytes) -> wasmtime_wasi_io::streams::StreamResult<()> {
        self.buffer.write(bytes)?;
        self.pending = true;
        self.started.take().unwrap().send(()).unwrap();
        Ok(())
    }
    fn flush(&mut self) -> wasmtime_wasi_io::streams::StreamResult<()> {
        if self.failed {
            Err(wasmtime_wasi_io::streams::StreamError::LastOperationFailed(
                std::io::Error::from(std::io::ErrorKind::BrokenPipe).into(),
            ))
        } else {
            Ok(())
        }
    }
}

#[test]
fn output_result_waits_for_flush_and_preserves_late_errors() {
    with_store(|mut store| {
        for fail in [false, true] {
            let output = MemoryOutputPipe::new(7);
            let (started, start) = oneshot::channel();
            let (release, released) = oneshot::channel();
            {
                let cli = &mut store.data_mut().ctx.as_mut().unwrap().cli;
                cli.stdin = Box::new(MemoryInputPipe::new(vec![1, 2, 3, 4, 5, 6, 7]));
                cli.stdout = Box::new(DelayedFactory(std::sync::Mutex::new(Some(DelayedOutput {
                    buffer: output.clone(),
                    started: Some(started),
                    release: Some(released),
                    pending: false,
                    fail,
                    failed: false,
                }))));
            }
            block_on(store.run_concurrent(async |accessor| {
                let cli = cli_accessor(accessor);
                let (data, input) = cli
                    .with(<WasiCli as stdin::HostWithStore<State>>::read_via_stream)
                    .unwrap();
                let written = cli
                    .with(|store| {
                        <WasiCli as stdout::HostWithStore<State>>::write_via_stream(store, data)
                    })
                    .unwrap();
                let input = receive(accessor, input);
                let mut written = receive(accessor, written);
                start.await.unwrap();
                assert_eq!(output.contents().as_ref(), [1, 2, 3, 4, 5, 6, 7]);
                assert!(
                    futures_lite::future::poll_once(&mut written)
                        .await
                        .is_none(),
                    "accepted bytes must not bypass pending flush"
                );
                release.send(()).unwrap();
                assert_eq!(
                    written.await.unwrap(),
                    if fail { Err(ErrorCode::Pipe) } else { Ok(()) }
                );
                assert_eq!(input.await.unwrap(), Ok(()));
            }))
            .unwrap();
            assert_eq!(output.contents().as_ref(), [1, 2, 3, 4, 5, 6, 7]);
            store.assert_concurrent_state_empty();
        }
    });
}

#[test]
fn canonical_component_transfers_stdio_streams_and_result_futures() {
    with_store(|mut store| {
        let expected: Vec<u8> = (0..(3 * 1024 * 1024 + 137))
            .map(|i| (i * 17 + 31) as u8)
            .collect();
        let output = MemoryOutputPipe::new(expected.len());
        {
            let cli = &mut store.data_mut().ctx.as_mut().unwrap().cli;
            cli.stdin = Box::new(MemoryInputPipe::new(expected.clone()));
            cli.stdout = Box::new(output.clone());
        }
        run_component(&mut store);
        assert_eq!(output.contents().as_ref(), expected);
        store.assert_concurrent_state_empty();
    });
}

fn run_component(store: &mut wasmtime::Store<State>) {
    use wasmtime::component::{Component, Linker};
    let component = Component::new(store.engine(), include_str!("streams.component.wat")).unwrap();
    let mut linker = Linker::<State>::new(store.engine());
    crate::p3::add_to_linker(&mut linker).unwrap();
    block_on(async {
        let instance = linker
            .instantiate_async(&mut *store, &component)
            .await
            .unwrap();
        let run = instance
            .get_typed_func::<(), ((FutureReader<Outcome>, FutureReader<Outcome>),)>(
                &mut *store,
                "run",
            )
            .unwrap();
        store
            .run_concurrent(async |accessor| {
                let ((input, output),) = run.call_concurrent(accessor, ()).await.unwrap();
                let input = receive(accessor, input);
                let output = receive(accessor, output);
                assert_eq!(input.await.unwrap(), Ok(()));
                assert_eq!(output.await.unwrap(), Ok(()));
            })
            .await
            .unwrap();
    });
}

#[test]
fn canonical_component_streams_real_inherited_stdio() {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    const FLAG: &str = "PERRY_P3_STDIO_CHILD";
    const BEGIN: &[u8] = b"PERRY-P3-BEGIN\n";
    const END: &[u8] = b"PERRY-P3-END\n";
    if std::env::var_os(FLAG).is_some() {
        with_store(|mut store| {
            let mut ctx = crate::WasiCtx::new(4).unwrap();
            ctx.cli.inherit_stdin();
            ctx.cli.inherit_stdout();
            store.data_mut().ctx = Some(ctx);
            std::io::stdout().write_all(BEGIN).unwrap();
            std::io::stdout().flush().unwrap();
            run_component(&mut store);
            store.assert_concurrent_state_empty();
            assert_eq!(store.data().ctx.as_ref().unwrap().outstanding_io(), 0);
            drop(store);
            // Context disposal must leave the process's original stdout alive.
            std::io::stdout().write_all(END).unwrap();
            std::io::stdout().flush().unwrap();
        });
        return;
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "p3::cli::tests::canonical_component_streams_real_inherited_stdio",
                "--nocapture",
                "--test-threads=1",
                "-Z",
                "unstable-options",
                "--force-run-in-process",
            ])
            .env(FLAG, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let expected: Vec<u8> = (0..(3 * 1024 * 1024 + 137))
        .map(|i| (i * 17 + 31) as u8)
        .collect();
    let payload = expected.clone();
    let mut stdin = child.0.stdin.take().unwrap();
    let mut stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    // Duplex transfer keeps either finite OS pipe from blocking its peer.
    let writer = std::thread::spawn(move || stdin.write_all(&payload));
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).unwrap();
        text
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "P3 native stdio child watchdog");
        std::thread::sleep(Duration::from_millis(10));
    };
    let input = writer.join().unwrap();
    let output = reader.join().unwrap();
    let errors = errors.join().unwrap();
    assert!(status.success(), "{errors}");
    input.unwrap();
    let begin = output
        .windows(BEGIN.len())
        .position(|part| part == BEGIN)
        .unwrap()
        + BEGIN.len();
    assert_eq!(&output[begin..begin + expected.len()], expected);
    assert_eq!(
        &output[begin + expected.len()..begin + expected.len() + END.len()],
        END
    );
}
