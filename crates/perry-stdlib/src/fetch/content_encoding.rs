//! HTTP content decoding for buffered Fetch responses.

use std::io::Read;

fn read_all(reader: impl Read) -> Option<Vec<u8>> {
    let mut decoded = Vec::new();
    let mut reader = reader;
    reader.read_to_end(&mut decoded).ok()?;
    Some(decoded)
}

fn decode_one(coding: &str, body: &[u8]) -> Option<Vec<u8>> {
    match coding {
        "gzip" | "x-gzip" => read_all(flate2::read::GzDecoder::new(body)),
        "deflate" | "x-deflate" => read_all(flate2::read::ZlibDecoder::new(body))
            .or_else(|| read_all(flate2::read::DeflateDecoder::new(body))),
        "br" => read_all(brotli::Decompressor::new(body, 4096)),
        "identity" | "" => Some(body.to_vec()),
        _ => None,
    }
}

fn decode_content_encoded_body(content_encoding: &str, body: &[u8]) -> Option<Vec<u8>> {
    let mut decoded = body.to_vec();
    for coding in content_encoding
        .split(',')
        .map(|coding| coding.trim().to_ascii_lowercase())
        .rev()
    {
        decoded = decode_one(&coding, &decoded)?;
    }
    Some(decoded)
}

/// Buffer and decode a reqwest response without enabling reqwest's automatic
/// decoder. The automatic decoder removes `Content-Encoding` and
/// `Content-Length`; Fetch keeps those response headers observable after body
/// decoding, as Node does.
pub(super) async fn response_body_bytes(response: reqwest::Response) -> Vec<u8> {
    let content_encoding = response
        .headers()
        .get_all(reqwest::header::CONTENT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(",");
    let body = response.bytes().await.unwrap_or_default().to_vec();
    decode_content_encoded_body(&content_encoding, &body).unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const PLAIN: &[u8] = br#"{"compressed":true}"#;

    #[test]
    fn decodes_gzip_deflate_and_brotli() {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(PLAIN).unwrap();
        let gzip = gzip.finish().unwrap();

        let mut deflate =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        deflate.write_all(PLAIN).unwrap();
        let deflate = deflate.finish().unwrap();

        let mut brotli = Vec::new();
        let mut encoder = brotli::CompressorReader::new(PLAIN, 4096, 5, 22);
        encoder.read_to_end(&mut brotli).unwrap();

        assert_eq!(decode_content_encoded_body("gzip", &gzip).unwrap(), PLAIN);
        assert_eq!(
            decode_content_encoded_body("deflate", &deflate).unwrap(),
            PLAIN
        );
        assert_eq!(decode_content_encoded_body("br", &brotli).unwrap(), PLAIN);
    }

    #[test]
    fn preserves_unknown_or_invalid_encodings() {
        let body = b"raw bytes";
        assert!(decode_content_encoded_body("zstd", body).is_none());
        assert!(decode_content_encoded_body("gzip", body).is_none());
    }
}
