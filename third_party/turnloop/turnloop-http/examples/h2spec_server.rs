//! Strict conformance server on turnloop's executor, with no blocking socket I/O.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use turnloop_http::{
        asynchronous::server::{self, Server},
        http1::Header,
        http2::Event,
    };
    use turnloop_io::turnloop::{Config, LocalExecutor, Timeout, backend::Platform};
    const BODY: [u8; 16384] = [b'x'; 16384];
    let mut executor = LocalExecutor::<Platform>::new(Config::default())?;
    let mut server = Server::bind(executor.handle(), "127.0.0.1:0".parse()?)?;
    println!("{}", server.local_addr()?.port());
    let task = executor.spawn_local(async move {
        server
            .run(|stream, signal| async move {
                let mut pending = Vec::<(u32, usize)>::new();
                server::http2(stream, signal, move |core, event| {
                    let mut reply = None;
                    match event {
                        Event::Headers {
                            stream,
                            end_stream: true,
                            ..
                        } => reply = Some(stream),
                        Event::Data {
                            stream,
                            bytes,
                            end_stream,
                        } => {
                            core.release_capacity(stream, bytes.len() as u32)
                                .map_err(std::io::Error::other)?;
                            if end_stream {
                                reply = Some(stream);
                            }
                        }
                        _ => {}
                    }
                    if let Some(id) = reply {
                        core.send_headers(
                            id,
                            &[
                                Header::new(":status", "200"),
                                Header::new("content-length", "16384"),
                            ],
                            false,
                        )
                        .map_err(std::io::Error::other)?;
                        pending.push((id, 0));
                    }
                    pending.retain_mut(|(id, offset)| {
                        match core.send_data(*id, &BODY[*offset..], true) {
                            Ok(n) => {
                                *offset += n;
                                *offset < BODY.len()
                            }
                            Err(_) => false,
                        }
                    });
                    Ok(())
                })
                .await
            })
            .await
    })?;
    while !task.is_finished() {
        executor.turn(Timeout::Forever)?;
    }
    Err("conformance server stopped unexpectedly".into())
}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn main() {
    eprintln!("Browser listening sockets are unsupported");
}
