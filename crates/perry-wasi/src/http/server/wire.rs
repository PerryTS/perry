use super::*;
use crate::http::{
    body::{BodyFrame, BodyReceiver, IncomingBody, StreamContext, CHUNK_SIZE},
    p2::bindings::http::types::Scheme,
    transport::{
        deadline,
        reader::{fields, ReadMessage},
    },
    FieldMap,
};
use futures_util::{
    future::{select, Either},
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
};
use turnloop::ExecutorHandle;
use turnloop_http::http1::{BodyLength, Encoder, Head, Header};

const HEAD_LIMIT: usize = 32 * 1024;

pub(super) async fn exchange(
    io: &ExecutorHandle<Platform>,
    stream: Transport<AsyncIo<Platform>>,
    options: &Options,
    send: oneshot::Sender<Result<Request, Error>>,
) -> Result<(), Error> {
    let (mut input, mut output) = stream.split();
    let result = async {
        let mut reader = ReadMessage::request(&mut input);
        let head = reader
            .head(io, after(io, options.request_head_timeout)?)
            .await;
        let request = head.and_then(|h| request(h, options.tls.is_some()));
        let (request, mut incoming, expect_continue) = match request {
            Ok(v) => v,
            Err(error) => {
                let _ = send.send(Err(error.clone()));
                let _ = write(
                    io,
                    &mut output,
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    options,
                )
                .await;
                return Err(error);
            }
        };
        if expect_continue {
            write(io, &mut output, b"HTTP/1.1 100 Continue\r\n\r\n", options).await?;
        }
        let method = request.method.clone();
        let (send_response, receive) = oneshot::channel();
        if send
            .send(Ok(Request {
                request,
                response: send_response,
            }))
            .is_err()
        {
            return Err(Error::ConnectionTerminated);
        }
        // One task drives both halves, even at admission limit one. An input
        // consumer disappearing switches to discard; it cannot cancel output.
        let read = std::pin::pin!(async {
            let result = reader
                .body(io, options.between_bytes_timeout, &mut incoming, true)
                .await;
            match result {
                Ok(trailers) if incoming.consumer_closed() => {
                    drop(trailers);
                    Ok(())
                }
                Ok(trailers) => incoming.finish(trailers),
                Err(e) => {
                    incoming.fail(e.clone());
                    Err(e)
                }
            }
        });
        let respond = std::pin::pin!(response(io, &mut output, receive, method, options));
        match select(respond, read).await {
            Either::Left((result, _)) => result,
            Either::Right((read_result, response)) => {
                let response_result = response.await;
                response_result.and(read_result)
            }
        }
    }
    .await;
    // Half-close first: closing with unread upload bytes can reset the peer and
    // lose an already-flushed early response. The host bounds the discard time.
    let mut stream = input
        .reunite(output)
        .map_err(|_| Error::ConnectionTerminated)?;
    let mut scratch = [0; 16384];
    let close = turnloop_io::linger_close(
        &mut stream,
        &mut scratch,
        Some(after(io, options.linger_timeout)?),
    )
    .await;
    result.and(close.map(|_| ()).map_err(|_| Error::ConnectionTerminated))
}

fn request(
    head: Head,
    tls: bool,
) -> Result<
    (
        HostIncomingRequest,
        crate::http::body::IncomingBodySender,
        bool,
    ),
    Error,
> {
    if head.version != 1 {
        return Err(Error::HttpProtocolError);
    }
    let method = http::Method::from_bytes(head.method.as_bytes())
        .map_err(|_| Error::HttpRequestMethodInvalid)?;
    if method == http::Method::CONNECT {
        return Err(Error::HttpRequestMethodInvalid);
    }
    // An origin server accepts origin-form and OPTIONS *, not forward-proxy
    // absolute-form. Exactly one syntactically valid Host is mandatory in 1.1.
    if !(head.target.starts_with('/') || head.target == "*" && method == http::Method::OPTIONS) {
        return Err(Error::HttpRequestUriInvalid);
    }
    let uri: http::Uri = head
        .target
        .parse()
        .map_err(|_| Error::HttpRequestUriInvalid)?;
    if uri.scheme().is_some() || uri.authority().is_some() || head.target.contains('#') {
        return Err(Error::HttpRequestUriInvalid);
    }
    let mut hosts = head
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("host"));
    let host = hosts.next().ok_or(Error::HttpRequestUriInvalid)?;
    if hosts.next().is_some() {
        return Err(Error::HttpRequestUriInvalid);
    }
    let authority = std::str::from_utf8(&host.value).map_err(|_| Error::HttpRequestUriInvalid)?;
    let parsed: http::uri::Authority = authority
        .parse()
        .map_err(|_| Error::HttpRequestUriInvalid)?;
    if parsed.host().is_empty() || authority.contains('@') {
        return Err(Error::HttpRequestUriInvalid);
    }
    let suffix = &authority[parsed.host().len()..];
    if !suffix.is_empty()
        && suffix
            .strip_prefix(':')
            .and_then(|p| {
                if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                    None
                } else {
                    p.parse::<u16>().ok()
                }
            })
            .is_none()
    {
        return Err(Error::HttpRequestUriInvalid);
    }
    let authority = authority.to_owned();
    let expects: Vec<_> = head
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("expect"))
        .collect();
    if expects.len() > 1
        || expects
            .first()
            .is_some_and(|h| !h.value.eq_ignore_ascii_case(b"100-continue"))
    {
        return Err(Error::HttpProtocolError);
    }
    let expect_continue = !expects.is_empty();
    let expected = match head.get("content-length") {
        Some(n) => Some(
            std::str::from_utf8(n)
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or(Error::HttpRequestBodySize(None))?,
        ),
        None if head.get("transfer-encoding").is_none() => Some(0),
        None => None,
    };
    let headers = fields(head.headers, StreamContext::Request)?;
    let (body, incoming) = IncomingBody::new(StreamContext::Request, expected);
    Ok((
        HostIncomingRequest {
            method,
            uri,
            scheme: if tls { Scheme::Https } else { Scheme::Http },
            authority,
            headers,
            body: Some(body),
        },
        incoming,
        expect_continue,
    ))
}

async fn write<W: AsyncWrite + Unpin>(
    io: &ExecutorHandle<Platform>,
    writer: &mut W,
    bytes: &[u8],
    options: &Options,
) -> Result<(), Error> {
    deadline(
        io,
        after(io, options.between_bytes_timeout)?,
        Error::ConnectionWriteTimeout,
        async {
            writer
                .write_all(bytes)
                .await
                .map_err(|_| Error::ConnectionTerminated)?;
            writer
                .flush()
                .await
                .map_err(|_| Error::ConnectionTerminated)
        },
    )
    .await
}

async fn response<W: AsyncWrite + Unpin>(
    io: &ExecutorHandle<Platform>,
    writer: &mut W,
    receive: oneshot::Receiver<Result<HostOutgoingResponse, Error>>,
    method: http::Method,
    options: &Options,
) -> Result<(), Error> {
    let prepared = deadline(
        io,
        after(io, options.response_head_timeout)?,
        Error::HttpResponseTimeout,
        async { receive.await.map_err(|_| Error::HttpResponseIncomplete)? },
    )
    .await
    .and_then(|response| prepare(response, &method));
    let (mut encoder, bytes, mut body, omitted) = match prepared {
        Ok(v) => v,
        Err(error) => {
            let _ = write(io, writer, b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", options).await;
            return Err(error);
        }
    };
    let result = async {
        write(io, writer, &bytes, options).await?;
        let mut buffer = Vec::with_capacity(CHUNK_SIZE + 32);
        let mut trailers = Vec::new();
        if let Some(body) = &mut body {
            loop {
                let frame = deadline(
                    io,
                    after(io, options.between_bytes_timeout)?,
                    Error::HttpResponseTimeout,
                    body.next(),
                )
                .await?;
                match frame {
                    Some(BodyFrame::Data(chunk)) => {
                        if omitted {
                            continue;
                        }
                        buffer.clear();
                        encoder
                            .body(&chunk, &mut buffer)
                            .map_err(|_| Error::HttpResponseBodySize(None))?;
                        write(io, writer, &buffer, options).await?;
                    }
                    Some(BodyFrame::Trailers(fields)) if !omitted => {
                        if FieldMap::content_size(&fields) > HEAD_LIMIT {
                            return Err(Error::HttpResponseTrailerSectionSize(None));
                        }
                        trailers = fields
                            .ordered_iter()
                            .map(|(n, v)| Header::new_preserving_case(n, v.as_bytes()))
                            .collect();
                    }
                    Some(BodyFrame::Trailers(_)) => {}
                    None => break,
                }
            }
        }
        buffer.clear();
        encoder
            .finish(&trailers, &mut buffer)
            .map_err(|_| Error::HttpResponseBodySize(None))?;
        write(io, writer, &buffer, options).await
    }
    .await;
    if let (Err(error), Some(body)) = (&result, &mut body) {
        body.fail(error.clone());
    }
    result
}

type Prepared = (Encoder, Vec<u8>, Option<BodyReceiver>, bool);
fn prepare(response: HostOutgoingResponse, method: &http::Method) -> Result<Prepared, Error> {
    let status = response.status.as_u16();
    if status < 200 {
        return Err(Error::HttpProtocolError);
    }
    if FieldMap::content_size(&response.headers) > HEAD_LIMIT {
        return Err(Error::HttpResponseHeaderSectionSize(None));
    }
    let omitted = *method == http::Method::HEAD || matches!(status, 204 | 304);
    let expected = response
        .headers
        .get("content-length")
        .map(|n| {
            n.to_str()
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or(Error::HttpResponseBodySize(None))
        })
        .transpose()?;
    if !omitted && response.body.is_none() && expected.is_some_and(|n| n != 0) {
        return Err(Error::HttpResponseBodySize(Some(0)));
    }
    let length = if omitted {
        BodyLength::Omitted
    } else if let Some(n) = expected {
        BodyLength::Known(n)
    } else if response.body.is_none() {
        BodyLength::Known(0)
    } else {
        BodyLength::Chunked
    };
    let mut headers: Vec<_> = response
        .headers
        .ordered_iter()
        .map(|(n, v)| Header::new_preserving_case(n, v.as_bytes()))
        .collect();
    headers.push(Header::new("connection", "close"));
    let head = Head {
        method: String::new(),
        target: String::new(),
        status,
        version: 1,
        headers,
        keep_alive: false,
    };
    let mut bytes = Vec::new();
    let encoder =
        Encoder::start(&head, length, &mut bytes).map_err(|_| Error::HttpProtocolError)?;
    if bytes.len() > HEAD_LIMIT {
        return Err(Error::HttpResponseHeaderSectionSize(None));
    }
    Ok((encoder, bytes, response.body, omitted))
}
