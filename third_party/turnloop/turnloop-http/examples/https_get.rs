//! HTTPS GET with streamed output. The application explicitly turns the executor.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        future::Future,
        io::Write,
        pin::Pin,
        task::{Context, Poll, Waker},
        time::{SystemTime, UNIX_EPOCH},
    };
    use turnloop_http::{
        asynchronous::client::{Client, Options},
        client::Request,
    };
    use turnloop_io::turnloop::{Config, LocalExecutor, Timeout, backend::Platform};
    use turnloop_tls::{
        ClientConfig, ClientOptions,
        rustls::pki_types::{CertificateDer, pem::PemObject},
    };
    let mut args = std::env::args().skip(1);
    let url = args.next().ok_or("usage: https_get URL [CA.pem]")?;
    let ca = args
        .next()
        .map(|path| CertificateDer::pem_file_iter(path).and_then(|certs| certs.collect()))
        .transpose()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let tls = ClientConfig::new(
        ClientOptions {
            ca,
            alpn: vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            ..Default::default()
        },
        now,
    )?;
    let mut executor = LocalExecutor::<Platform>::new(Config::default())?;
    let h = executor.handle();
    let mut task = executor.spawn_local(async move {
        let mut client = Client::new(h, tls, now, Options::default());
        let mut request = Request::new(&url, "GET").map_err(std::io::Error::other)?;
        let head = client
            .request(&mut request, |bytes| std::io::stdout().write_all(bytes))
            .await?;
        eprintln!("HTTP {}", head.status);
        Ok::<_, std::io::Error>(())
    })?;
    while !task.is_finished() {
        executor.turn(Timeout::Forever)?;
    }
    match Pin::new(&mut task).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(Ok(result)) => result?,
        _ => return Err("request cancelled".into()),
    };
    Ok(())
}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn main() {
    eprintln!("Use the browser host-fetch adapter");
}
