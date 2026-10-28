//! HTTP/1 and HTTP/2 echo over TLS, selected by negotiated ALPN.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use turnloop_http::{
        asynchronous::server::{self, Server},
        http1::{BodyLength, Event, Head, Header},
        http2,
    };
    use turnloop_io::turnloop::{Config, LocalExecutor, Timeout, backend::Platform};
    use turnloop_tls::{
        ServerConfig, TlsStream,
        rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    };
    let mut args = std::env::args().skip(1);
    let cert = args
        .next()
        .ok_or("usage: tls_echo CERT.pem KEY.pem [ADDRESS]")?;
    let key = args.next().ok_or("missing key")?;
    let address = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:8443".into())
        .parse()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let tls = ServerConfig::new(
        CertificateDer::pem_file_iter(cert)?.collect::<Result<Vec<_>, _>>()?,
        PrivateKeyDer::from_pem_file(key)?,
        vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        now,
    )?;
    let mut executor = LocalExecutor::<Platform>::new(Config::default())?;
    let h = executor.handle();
    let mut server = Server::bind(h.clone(), address)?;
    println!("{}", server.local_addr()?);
    let task = executor.spawn_local(async move {
        server
            .run(move |stream, signal| {
                let h = h.clone();
                let tls = tls.clone();
                async move {
                    let stream =
                        TlsStream::accept(stream, &tls, &h, h.now() + Duration::from_secs(10), now)
                            .await?;
                    if stream.alpn_protocol() == Some(b"h2") {
                        let mut pending = Vec::<(u32, Vec<u8>, usize, bool)>::with_capacity(16);
                        let mut buffers = Vec::<Vec<u8>>::with_capacity(16);
                        server::http2(stream, signal, move |core, event| {
                            match event {
                                http2::Event::Headers {
                                    stream, end_stream, ..
                                } => {
                                    core.send_headers(
                                        stream,
                                        &[Header::new(":status", "200")],
                                        end_stream,
                                    )
                                    .map_err(std::io::Error::other)?;
                                }
                                http2::Event::Data {
                                    stream,
                                    bytes,
                                    end_stream,
                                } => {
                                    let mut buffer =
                                        buffers.pop().unwrap_or_else(|| Vec::with_capacity(16384));
                                    buffer.extend_from_slice(bytes);
                                    pending.push((stream, buffer, 0, end_stream));
                                }
                                _ => {}
                            }
                            let mut index = 0;
                            while index < pending.len() {
                                let (id, bytes, offset, end) = &mut pending[index];
                                let n = core
                                    .send_data(*id, &bytes[*offset..], *end)
                                    .map_err(std::io::Error::other)?;
                                *offset += n;
                                core.release_capacity(*id, n as u32)
                                    .map_err(std::io::Error::other)?;
                                if *offset == bytes.len() {
                                    let (_, mut buffer, _, _) = pending.remove(index);
                                    buffer.clear();
                                    buffers.push(buffer);
                                } else {
                                    index += 1;
                                }
                            }
                            Ok(())
                        })
                        .await
                    } else {
                        server::http1(stream, signal, |event, out| {
                            match event {
                                Event::Head(_) => out.start(
                                    &Head {
                                        method: String::new(),
                                        target: String::new(),
                                        status: 200,
                                        version: 1,
                                        headers: vec![],
                                        keep_alive: true,
                                    },
                                    BodyLength::Chunked,
                                )?,
                                Event::Body(bytes) => out.body(bytes)?,
                                Event::End => out.finish(&[])?,
                                _ => {}
                            }
                            Ok(())
                        })
                        .await
                    }
                }
            })
            .await
    })?;
    while !task.is_finished() {
        executor.turn(Timeout::Forever)?;
    }
    Ok(())
}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn main() {
    eprintln!("Browser listening sockets are unsupported");
}
