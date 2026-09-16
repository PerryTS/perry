//! The tokio transport: [`crate::codec::Codec`] driven over any byte stream.
//!
//! This is the *declining* transport. A connection whose agent owns a
//! `turnloop::Loop` is driven by [`crate::turnloop_link`] instead, with no task
//! and no channel. Both drive the same codec, which is the point: the protocol
//! moved out of the transport crate, so a transport swap is now a change of
//! who calls `receive`/`take_output` and nothing else.
//!
//! What this replaces is `tokio_tungstenite::WebSocketStream::split()` plus a
//! `futures_util` `Sink`/`Stream` pair. The stream is now split by
//! `tokio::io::split`, which works for any `AsyncRead + AsyncWrite` — including
//! `TokioIo<hyper::upgrade::Upgraded>`, which is what made
//! `register_external_ws_stream` generic in the first place.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::codec::{Codec, Incoming};
use crate::{
    connection_closed, connection_error, emit_incoming, WsCommand,
};

/// One read's worth of wire bytes. Matches tungstenite's own default read
/// buffer, so a large message costs the same number of syscalls it used to.
const READ_CHUNK: usize = 128 * 1024;

/// Anything this transport can carry. The blanket impl is what lets the HTTP
/// upgrade path hand over `TokioIo<Upgraded>` without naming it here.
pub trait Transport: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {}
impl<T> Transport for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static
{}

/// Drive a connection until it closes.
///
/// `leftover` is whatever arrived in the same read as the handshake response —
/// already frame data, and dropping it loses the peer's first message.
pub(crate) async fn run<S: Transport>(
    ws_id: usize,
    stream: S,
    mut codec: Codec,
    leftover: Vec<u8>,
    mut rx: mpsc::UnboundedReceiver<WsCommand>,
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut buffer = vec![0u8; READ_CHUNK];
    let mut closed_with: Option<(u16, String)> = None;

    // The leftover has to go through the codec before the first read, or a
    // message that arrived with the 101 is delivered out of order.
    if !leftover.is_empty()
        && !feed(ws_id, &mut codec, &leftover, &mut writer, &mut closed_with).await
    {
        finish(ws_id, closed_with);
        return;
    }

    loop {
        if codec.is_terminal() {
            break;
        }
        tokio::select! {
            read = reader.read(&mut buffer) => match read {
                Ok(0) => {
                    // EOF without a close frame is 1006, with one it is the
                    // peer's own code — `Codec::eof` knows which.
                    if let Some(code) = codec.eof() {
                        closed_with.get_or_insert((code, String::new()));
                    }
                    break;
                }
                Ok(n) => {
                    if !feed(ws_id, &mut codec, &buffer[..n], &mut writer, &mut closed_with).await {
                        break;
                    }
                }
                Err(e) => {
                    connection_error(ws_id, &e.to_string());
                    closed_with.get_or_insert((crate::codec::CLOSE_ABNORMAL, String::new()));
                    break;
                }
            },
            command = rx.recv() => match command {
                Some(command) => {
                    if !apply(ws_id, &mut codec, command, &mut writer, &mut closed_with).await {
                        break;
                    }
                }
                // Every sender dropped: the JS object is unreachable.
                None => break,
            },
        }
    }

    let _ = writer.shutdown().await;
    finish(ws_id, closed_with);
}

/// Feed wire bytes through the codec, emit what they decoded, flush what the
/// codec wants to answer. `false` means the connection is finished.
async fn feed<W: tokio::io::AsyncWrite + Unpin>(
    ws_id: usize,
    codec: &mut Codec,
    bytes: &[u8],
    writer: &mut W,
    closed_with: &mut Option<(u16, String)>,
) -> bool {
    let events = match codec.receive(bytes) {
        Ok(events) => events,
        Err(e) => {
            connection_error(ws_id, &crate::codec_error_message(&e));
            closed_with.get_or_insert((crate::codec::CLOSE_ABNORMAL, String::new()));
            // Still flush: the codec may have queued a close frame naming the
            // protocol error, and `ws` sends it.
            let _ = flush(codec, writer).await;
            return false;
        }
    };
    let mut done = false;
    for event in events {
        if let Incoming::Close(frame) = &event {
            let (code, reason) = frame
                .clone()
                .unwrap_or((crate::codec::CLOSE_NO_STATUS, String::new()));
            *closed_with = Some((code, reason));
            done = true;
        }
        emit_incoming(ws_id, event);
    }
    if !flush(codec, writer).await {
        return false;
    }
    !done
}

async fn apply<W: tokio::io::AsyncWrite + Unpin>(
    ws_id: usize,
    codec: &mut Codec,
    command: WsCommand,
    writer: &mut W,
    closed_with: &mut Option<(u16, String)>,
) -> bool {
    match command {
        WsCommand::Send(outgoing) => {
            if let Err(e) = codec.send(outgoing.into_message()) {
                connection_error(ws_id, &crate::codec_error_message(&e));
                return false;
            }
        }
        WsCommand::Close(code, reason) => {
            if let Err(e) = codec.close(code, &reason) {
                connection_error(ws_id, &crate::codec_error_message(&e));
                return false;
            }
            // Do not break here: `ws.close()` starts the closing handshake and
            // the connection stays open until the peer answers or the codec's
            // close deadline fires. Breaking would be `terminate()`.
        }
        WsCommand::Terminate => {
            closed_with.get_or_insert((crate::codec::CLOSE_ABNORMAL, String::new()));
            return false;
        }
    }
    flush(codec, writer).await
}

async fn flush<W: tokio::io::AsyncWrite + Unpin>(codec: &mut Codec, writer: &mut W) -> bool {
    let out = codec.take_output();
    if out.is_empty() {
        return true;
    }
    writer.write_all(&out).await.is_ok()
}

fn finish(ws_id: usize, closed_with: Option<(u16, String)>) {
    let (code, reason) = closed_with.unwrap_or((crate::codec::CLOSE_ABNORMAL, String::new()));
    connection_closed(ws_id, code, reason);
}
