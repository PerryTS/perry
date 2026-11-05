use super::reader::{fields, ReadMessage};
use super::*;
use crate::http::body::{BodyFrame, StreamContext, CHUNK_SIZE};
use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use turnloop_http::http1::{BodyLength, Encoder, Head, Header};

const HEAD_LIMIT: usize = 32 * 1024;

pub(super) async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    io: &ExecutorHandle<Platform>,
    stream: S,
    endpoint: Endpoint,
    spec: RequestSpec,
    body: &mut BodyReceiver,
    options: Options,
    send: oneshot::Sender<Result<Response, Error>>,
) -> Result<(), Error> {
    let (mut reader, mut writer) = stream.split();
    let prepared = prepare(&endpoint, &spec);
    let (mut encoder, bytes) = match prepared {
        Ok(v) => v,
        Err(error) => {
            body.fail(error.clone());
            let _ = send.send(Err(error.clone()));
            return Err(error);
        }
    };
    if let Err(error) = write(io, &mut writer, &bytes, &options).await {
        body.fail(error.clone());
        let _ = send.send(Err(error.clone()));
        return Err(error);
    }
    let mut reader = ReadMessage::response(&mut reader, &spec.method);
    let first = match after(io, options.first_byte_timeout) {
        Ok(at) => at,
        Err(error) => {
            body.fail(error.clone());
            let _ = send.send(Err(error.clone()));
            return Err(error);
        }
    };
    // Both directions are polled by this single owned task. The stream halves
    // share only a per-poll lock; a pending native read never locks out a write.
    let (head, uploaded) = {
        let upload = std::pin::pin!(upload(io, &mut writer, &mut encoder, body, &options));
        let response = std::pin::pin!(reader.head(io, first));
        match select(response, upload).await {
            Either::Left((head, _upload)) => (head, false),
            Either::Right((Ok(()), response)) => (response.await, true),
            Either::Right((Err(error), _response)) => (Err(error), false),
        }
    };
    let head = match head {
        Ok(head) => head,
        Err(error) => {
            body.fail(error.clone());
            let _ = send.send(Err(error.clone()));
            return Err(error);
        }
    };
    // A final response can reject a body before the guest finishes writing it.
    // Close the upload side immediately and keep receiving the response body.
    if !uploaded {
        body.fail(Error::ConnectionTerminated);
    }
    let no_body = spec.method == "HEAD" || matches!(head.status, 204 | 304);
    let expected = if no_body {
        Some(0)
    } else {
        head.get("content-length")
            .and_then(|n| std::str::from_utf8(n).ok()?.parse().ok())
    };
    let headers = match fields(head.headers, StreamContext::Response) {
        Ok(fields) => fields,
        Err(e) => {
            let _ = send.send(Err(e.clone()));
            return Err(e);
        }
    };
    let (body, mut output) = IncomingBody::new(StreamContext::Response, expected);
    if send
        .send(Ok(Response {
            status: head.status,
            headers,
            body,
        }))
        .is_err()
    {
        return Err(Error::ConnectionTerminated);
    }
    match reader
        .body(io, options.between_bytes_timeout, &mut output, false)
        .await
    {
        Ok(trailers) => {
            output.finish(trailers)?;
            if uploaded {
                Ok(())
            } else {
                Err(Error::ConnectionTerminated)
            }
        }
        Err(error) => {
            output.fail(error.clone());
            Err(error)
        }
    }
}

fn prepare(endpoint: &Endpoint, spec: &RequestSpec) -> Result<(Encoder, Vec<u8>), Error> {
    if FieldMap::content_size(&spec.headers) > HEAD_LIMIT {
        return Err(Error::HttpRequestHeaderSectionSize(None));
    }
    let mut headers: Vec<_> = spec
        .headers
        .ordered_iter()
        .map(|(name, value)| Header::new_preserving_case(name, value.as_bytes()))
        .collect();
    headers.push(Header::new("host", &endpoint.authority));
    // One exchange owns one connection. Reuse would need a capability-scoped
    // pool plus independent idle lifetime accounting, and is not implicit here.
    headers.push(Header::new("connection", "close"));
    let length = match spec.headers.get("content-length") {
        Some(value) => BodyLength::Known(
            value
                .to_str()
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or(Error::HttpRequestBodySize(None))?,
        ),
        None => BodyLength::Chunked,
    };
    let head = Head {
        method: spec.method.clone(),
        target: endpoint.path.clone(),
        status: 0,
        version: 1,
        headers,
        keep_alive: false,
    };
    let mut bytes = Vec::new();
    let encoder =
        Encoder::start(&head, length, &mut bytes).map_err(|_| Error::HttpProtocolError)?;
    if bytes.len() > HEAD_LIMIT {
        return Err(Error::HttpRequestHeaderSectionSize(None));
    }
    Ok((encoder, bytes))
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
async fn upload<W: AsyncWrite + Unpin>(
    io: &ExecutorHandle<Platform>,
    writer: &mut W,
    encoder: &mut Encoder,
    body: &mut BodyReceiver,
    options: &Options,
) -> Result<(), Error> {
    let mut bytes = Vec::with_capacity(CHUNK_SIZE + 32);
    let mut trailers = Vec::new();
    while let Some(frame) = body.next().await? {
        match frame {
            BodyFrame::Data(chunk) => {
                bytes.clear();
                encoder
                    .body(&chunk, &mut bytes)
                    .map_err(|_| Error::HttpRequestBodySize(None))?;
                write(io, writer, &bytes, options).await?;
            }
            BodyFrame::Trailers(fields) => {
                if FieldMap::content_size(&fields) > HEAD_LIMIT {
                    return Err(Error::HttpRequestTrailerSectionSize(None));
                }
                trailers = fields
                    .ordered_iter()
                    .map(|(n, v)| Header::new_preserving_case(n, v.as_bytes()))
                    .collect();
            }
        }
    }
    bytes.clear();
    encoder
        .finish(&trailers, &mut bytes)
        .map_err(|_| Error::HttpProtocolError)?;
    write(io, writer, &bytes, options).await
}
