#![allow(dead_code)]
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::Duration,
};
use turnloop_tls::{Client, ConnectionState, Server, UnbufferedStatus};
pub const NOW: u64 = 1_789_344_000;
pub trait Endpoint {
    type Data;
    fn process<'c, 'i>(
        &'c mut self,
        input: &'i mut [u8],
        time: u64,
    ) -> UnbufferedStatus<'c, 'i, Self::Data>;
}
impl Endpoint for Client {
    type Data = turnloop_tls::rustls::client::ClientConnectionData;
    fn process<'c, 'i>(
        &'c mut self,
        input: &'i mut [u8],
        time: u64,
    ) -> UnbufferedStatus<'c, 'i, Self::Data> {
        self.process(input, time)
    }
}
impl Endpoint for Server {
    type Data = turnloop_tls::rustls::server::ServerConnectionData;
    fn process<'c, 'i>(
        &'c mut self,
        input: &'i mut [u8],
        time: u64,
    ) -> UnbufferedStatus<'c, 'i, Self::Data> {
        self.process(input, time)
    }
}
/// Blocking test adapter only. No socket or clock is stored in a protocol crate.
pub struct Stream<E: Endpoint> {
    pub engine: E,
    socket: TcpStream,
    input: Vec<u8>,
    output: Vec<u8>,
    plain: Vec<u8>,
    pos: usize,
}
impl<E: Endpoint> Stream<E> {
    pub fn new(engine: E, socket: TcpStream) -> Self {
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Self {
            engine,
            socket,
            input: Vec::new(),
            output: Vec::new(),
            plain: Vec::new(),
            pos: 0,
        }
    }
    fn advance(&mut self, app: Option<&[u8]>) -> io::Result<bool> {
        let status = self.engine.process(&mut self.input, NOW);
        let mut discard = status.discard;
        let mut need_input = false;
        let mut wrote = false;
        match status.state.map_err(io::Error::other)? {
            ConnectionState::EncodeTlsData(mut encode) => {
                let start = self.output.len();
                self.output.resize(start + 65536, 0);
                let n = match encode.encode(&mut self.output[start..]) {
                    Ok(n) => n,
                    Err(turnloop_tls::rustls::unbuffered::EncodeError::InsufficientSize(e)) => {
                        self.output.resize(start + e.required_size, 0);
                        encode
                            .encode(&mut self.output[start..])
                            .map_err(io::Error::other)?
                    }
                    Err(e) => return Err(io::Error::other(e)),
                };
                self.output.truncate(start + n);
            }
            ConnectionState::TransmitTlsData(tx) => {
                self.socket.write_all(&self.output)?;
                self.output.clear();
                tx.done();
            }
            ConnectionState::ReadTraffic(mut read) => {
                while let Some(record) = read.next_record() {
                    let record = record.map_err(io::Error::other)?;
                    discard += record.discard;
                    self.plain.extend_from_slice(record.payload);
                }
            }
            ConnectionState::WriteTraffic(mut write) => {
                if let Some(app) = app {
                    let mut out = [0u8; 65536];
                    let n = write.encrypt(app, &mut out).map_err(io::Error::other)?;
                    self.socket.write_all(&out[..n])?;
                    wrote = true;
                } else {
                    need_input = true;
                }
            }
            ConnectionState::BlockedHandshake => need_input = true,
            ConnectionState::PeerClosed | ConnectionState::Closed => return Ok(false),
            other => return Err(io::Error::other(format!("unexpected TLS state {other:?}"))),
        }
        self.input.drain(..discard);
        if need_input {
            // Deliberately fragmented input, including split record headers.
            let mut buf = [0; 137];
            let n = self.socket.read(&mut buf)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "TLS EOF without close_notify",
                ));
            }
            self.input.extend_from_slice(&buf[..n]);
        }
        Ok(wrote)
    }
}
impl<E: Endpoint> Read for Stream<E> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.pos == self.plain.len() {
            self.plain.clear();
            self.pos = 0;
            self.advance(None)?;
        }
        let n = out.len().min(self.plain.len() - self.pos);
        out[..n].copy_from_slice(&self.plain[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
impl<E: Endpoint> Write for Stream<E> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = bytes.len().min(16384);
        while !self.advance(Some(&bytes[..n]))? {}
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
pub fn certificate() -> rcgen::CertifiedKey {
    rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap()
}
pub fn server_config(cert: &rcgen::CertifiedKey) -> turnloop_tls::ServerConfig {
    turnloop_tls::ServerConfig::new(
        vec![cert.cert.der().clone()],
        turnloop_tls::rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der())
            .into(),
        vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        NOW,
    )
    .unwrap()
}
