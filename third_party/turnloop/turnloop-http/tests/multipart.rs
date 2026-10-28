use turnloop_http::{
    client::Request,
    http1::Header,
    multipart::{Form, Part},
};

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

#[test]
fn multipart_body_is_framed_byte_for_byte() {
    let form = Form::new()
        .text("version", "1.2.0")
        .text("say \"hi\"\r\n", "line1\r\nline2")
        .file("tarball", "pkg.tgz", vec![0x1f, 0x8b, 0, 255])
        .part(Part::file("manifest", "p.json", b"{}".to_vec()).content_type("application/json"));
    let encoded = form.encode([0x5a; 16]).unwrap();
    let b = &encoded.boundary;
    assert!(b.starts_with("turnloop-") && b.len() == 41);
    assert!(b[9..].bytes().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(
        encoded.content_type(),
        format!("multipart/form-data; boundary={b}")
    );
    let mut expected = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"version\"\r\n\r\n1.2.0\r\n\
         --{b}\r\nContent-Disposition: form-data; name=\"say %22hi%22%0D%0A\"\r\n\r\n\
         line1\r\nline2\r\n\
         --{b}\r\nContent-Disposition: form-data; name=\"tarball\"; filename=\"pkg.tgz\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    expected.extend_from_slice(&[0x1f, 0x8b, 0, 255]);
    expected.extend_from_slice(
        format!(
            "\r\n--{b}\r\nContent-Disposition: form-data; name=\"manifest\"; \
             filename=\"p.json\"\r\nContent-Type: application/json\r\n\r\n{{}}\r\n--{b}--\r\n"
        )
        .as_bytes(),
    );
    assert_eq!(encoded.body, expected);
    // The same entropy gives the same body; other entropy another boundary.
    assert_eq!(form.encode([0x5a; 16]).unwrap(), encoded);
    assert_ne!(form.encode([0x5b; 16]).unwrap().boundary, *b);

    let mut request = Request::new("https://registry.test/publish", "POST").unwrap();
    request
        .headers
        .push(Header::new("Content-Type", "text/plain"));
    let content_type = encoded.content_type();
    encoded.clone().apply(&mut request);
    let types: Vec<_> = request
        .headers
        .iter()
        .filter(|h| h.name == "content-type")
        .map(|h| h.value.clone())
        .collect();
    assert_eq!(types, [content_type.into_bytes()]);
    assert_eq!(request.body, encoded.body);
}

#[test]
fn the_chosen_boundary_never_occurs_inside_a_part() {
    let entropy = [0x42; 16];
    // With no parts nothing can collide, so this is the first candidate.
    let first = Form::new().encode(entropy).unwrap().boundary;
    let second = Form::new()
        .file("f", "x", first.clone().into_bytes())
        .encode(entropy)
        .unwrap()
        .boundary;
    assert_ne!(first, second);
    // Plant both candidates in a base64-looking field and the first again in a
    // filename: every candidate so far has to be skipped.
    let payload = format!("QUJD{first}REVG{second}R0hJ");
    let form = Form::new().text("signature", payload.clone()).file(
        "upload",
        format!("{first}.tgz"),
        b"data".to_vec(),
    );
    let encoded = form.encode(entropy).unwrap();
    let chosen = encoded.boundary.as_bytes();
    assert_ne!(chosen, first.as_bytes());
    assert_ne!(chosen, second.as_bytes());
    let mut delimiter = b"--".to_vec();
    delimiter.extend_from_slice(chosen);
    // One opening delimiter per part and the closing one, nothing else.
    assert_eq!(count(&encoded.body, &delimiter), 3);
    let mut closing = delimiter.clone();
    closing.extend_from_slice(b"--\r\n");
    assert!(encoded.body.ends_with(&closing));
    assert_eq!(count(&encoded.body, payload.as_bytes()), 1);
    assert_eq!(count(&encoded.body, first.as_bytes()), 2);
}

#[test]
fn multipart_rejects_header_injection_in_content_type() {
    let error = Form::new()
        .part(Part::text("a", "b").content_type("text/plain\r\nX-Evil: 1"))
        .encode([0; 16])
        .unwrap_err();
    assert_eq!(error.code, "UND_ERR_INVALID_ARG");
}
