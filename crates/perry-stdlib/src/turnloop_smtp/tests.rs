//! P6 acceptance for the SMTP client engine.
//!
//! `turnloop_smtp::Connection` is sans-I/O, so a full session — greeting, EHLO,
//! capability parsing, AUTH, MAIL/RCPT/DATA, dot-stuffing, QUIT — can be driven
//! here with real protocol bytes and no socket. That is the part of this phase
//! that decides whether mail is delivered correctly; the transport underneath
//! it is P1's, already tested there.

use std::time::Instant;

use turnloop_smtp::{Auth, Config, Connection, Envelope, Event, State, Tls};

/// Take everything the connection wants to write, as text.
fn drain(conn: &mut Connection) -> String {
    let out = String::from_utf8_lossy(conn.output()).into_owned();
    let n = conn.output().len();
    conn.consume_output(n);
    out
}

fn feed(conn: &mut Connection, text: &str) {
    conn.receive(text.as_bytes(), Instant::now())
        .unwrap_or_else(|e| panic!("receive {text:?}: {e:?}"));
}

fn events(conn: &mut Connection) -> Vec<Event> {
    let mut out = Vec::new();
    while let Some(event) = conn.poll_event() {
        out.push(event);
    }
    out
}

fn plain_config() -> Config {
    Config {
        name: "[127.0.0.1]".into(),
        tls: Tls::None,
        auth: Some(Auth::Plain {
            user: "probe".into(),
            password: "secret".into(),
        }),
        ..Config::default()
    }
}

const EHLO_REPLY: &str = "250-perry-test\r\n250-PIPELINING\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250-SIZE 10485760\r\n250 AUTH PLAIN LOGIN\r\n";

/// The whole delivery, asserted command by command. A test that only checked
/// "no error" would pass against a connection that sent nothing at all.
#[test]
fn a_full_delivery_writes_the_commands_in_order() {
    let mut conn = Connection::new(plain_config()).expect("config");
    conn.connected(Instant::now()).expect("connected");
    assert_eq!(conn.state(), State::Greeting);

    feed(&mut conn, "220 perry-test ESMTP\r\n");
    assert_eq!(drain(&mut conn), "EHLO [127.0.0.1]\r\n");

    feed(&mut conn, EHLO_REPLY);
    assert!(conn.capabilities().auth_plain, "AUTH PLAIN was advertised");
    assert!(conn.capabilities().eight_bit_mime);
    assert_eq!(conn.capabilities().size, Some(10_485_760));
    let auth = drain(&mut conn);
    assert!(
        auth.starts_with("AUTH PLAIN "),
        "credentials must be offered once the server advertises them: {auth:?}"
    );

    feed(&mut conn, "235 2.7.0 Authentication successful\r\n");
    assert!(
        events(&mut conn).iter().any(|e| matches!(e, Event::Ready)),
        "authentication completing is what makes the connection Ready"
    );
    assert_eq!(conn.state(), State::Ready);

    let envelope = Envelope {
        from: "sender@example.com".into(),
        to: vec!["a@example.com".into(), "b@example.com".into()],
    };
    conn.send(
        7,
        envelope.clone(),
        "<id@perry>".into(),
        b"Subject: x\r\n\r\nbody\r\n",
        Instant::now(),
    )
    .expect("send accepted");
    // The server advertised PIPELINING, so MAIL FROM, both RCPT TOs and DATA
    // are written as ONE block rather than one round trip each. Asserting per
    // command would be asserting the absence of pipelining.
    let mail = drain(&mut conn);
    assert!(
        mail.starts_with("MAIL FROM:<sender@example.com> SIZE="),
        "SIZE is advertised, so it must be declared: {mail:?}"
    );
    // MAIL FROM and both RCPT TOs go out together; DATA waits for their
    // replies, because its own reply (354) must not be confused with theirs.
    let order: Vec<usize> = [
        "MAIL FROM:<sender@example.com>",
        "RCPT TO:<a@example.com>",
        "RCPT TO:<b@example.com>",
    ]
    .iter()
    .map(|needle| {
        mail.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from {mail:?}"))
    })
    .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "the pipelined block must be in protocol order: {mail:?}"
    );
    assert!(
        !mail.contains("DATA\r\n"),
        "DATA must wait for the recipient replies: {mail:?}"
    );

    feed(&mut conn, "250 2.1.0 Ok\r\n");
    feed(&mut conn, "250 2.1.5 Ok\r\n");
    feed(&mut conn, "250 2.1.5 Ok\r\n");
    assert_eq!(drain(&mut conn), "DATA\r\n");
    feed(&mut conn, "354 Go ahead\r\n");
    let body = drain(&mut conn);
    assert!(
        body.ends_with("\r\n.\r\n"),
        "the body must be terminated: {body:?}"
    );

    feed(&mut conn, "250 2.0.0 Ok: queued as ABC123\r\n");
    let sent = events(&mut conn);
    let Some(Event::Sent { token, info }) = sent.into_iter().next() else {
        panic!("a completed DATA must produce Sent");
    };
    assert_eq!(token, 7, "the caller's token comes back");
    assert_eq!(info.accepted, vec!["a@example.com", "b@example.com"]);
    assert!(info.rejected.is_empty());
    assert_eq!(info.response_code, 250);
    assert!(info.response.contains("ABC123"), "{:?}", info.response);
    assert_eq!(info.envelope, envelope);
}

/// A line that begins with `.` must be stuffed, or the message ends early and
/// the rest of it is interpreted as commands. Asserted on the bytes.
#[test]
fn a_leading_dot_is_stuffed() {
    let mut out = Vec::new();
    turnloop_smtp::encode_data(b"before\r\n.\r\n.hidden\r\nafter\r\n", &mut out);
    let text = String::from_utf8(out).expect("ascii");
    assert!(text.contains("\r\n..\r\n"), "a bare dot line: {text:?}");
    assert!(
        text.contains("\r\n..hidden\r\n"),
        "a dot-prefixed line: {text:?}"
    );
    assert!(text.ends_with("\r\n.\r\n"), "terminator: {text:?}");
    assert!(
        !text.contains("\r\n.\r\nafter"),
        "the terminator must not appear inside the body: {text:?}"
    );
}

/// A rejected recipient is reported, not silently dropped — and a delivery in
/// which EVERY recipient was rejected fails rather than reporting success.
#[test]
fn a_rejected_recipient_is_reported() {
    let mut conn = ready_connection();
    conn.send(
        1,
        Envelope {
            from: "s@example.com".into(),
            to: vec!["nobody@example.com".into()],
        },
        "<id@perry>".into(),
        b"Subject: x\r\n\r\nbody\r\n",
        Instant::now(),
    )
    .expect("send accepted");
    drain(&mut conn);
    feed(&mut conn, "250 2.1.0 Ok\r\n");
    drain(&mut conn);
    feed(&mut conn, "550 5.1.1 No such user\r\n");
    let produced = events(&mut conn);
    assert!(
        produced
            .iter()
            .any(|e| matches!(e, Event::Failed { rejected, .. } if rejected.len() == 1)),
        "every recipient rejected must fail the delivery: {produced:?}"
    );
}

/// STARTTLS: the connection asks the host to upgrade, and re-issues EHLO on the
/// secure channel — a capability list learned in the clear must not be trusted.
#[test]
fn starttls_re_issues_ehlo_on_the_secure_channel() {
    let config = Config {
        name: "[127.0.0.1]".into(),
        tls: Tls::Required,
        auth: None,
        ..Config::default()
    };
    let mut conn = Connection::new(config).expect("config");
    conn.connected(Instant::now()).expect("connected");
    feed(&mut conn, "220 perry-test ESMTP\r\n");
    assert_eq!(drain(&mut conn), "EHLO [127.0.0.1]\r\n");
    feed(
        &mut conn,
        "250-perry-test\r\n250-STARTTLS\r\n250 AUTH PLAIN\r\n",
    );
    assert_eq!(drain(&mut conn), "STARTTLS\r\n");
    feed(&mut conn, "220 2.0.0 Ready to start TLS\r\n");
    assert!(
        events(&mut conn)
            .iter()
            .any(|e| matches!(e, Event::UpgradeTls)),
        "the host is asked to install the session; the protocol cannot do it"
    );
    assert_eq!(conn.state(), State::Tls);

    conn.tls_established(Instant::now()).expect("established");
    assert_eq!(
        drain(&mut conn),
        "EHLO [127.0.0.1]\r\n",
        "the cleartext capability list must be discarded and re-learned"
    );
    assert!(
        !conn.capabilities().starttls,
        "capabilities are reset across the upgrade"
    );
}

/// Implicit TLS (`secure: true`) upgrades before a single protocol byte.
#[test]
fn implicit_tls_upgrades_before_the_greeting() {
    let config = Config {
        name: "[127.0.0.1]".into(),
        tls: Tls::Implicit,
        auth: None,
        ..Config::default()
    };
    let mut conn = Connection::new(config).expect("config");
    conn.connected(Instant::now()).expect("connected");
    assert_eq!(conn.state(), State::Tls);
    assert!(events(&mut conn)
        .iter()
        .any(|e| matches!(e, Event::UpgradeTls)));
    assert!(
        conn.output().is_empty(),
        "nothing may be written in the clear on an implicit-TLS connection"
    );
    conn.tls_established(Instant::now()).expect("established");
    assert_eq!(conn.state(), State::Greeting);
}

/// A server that answers 421 has gone away; that must surface as a failure
/// rather than as a connection the engine keeps feeding.
#[test]
fn a_421_ends_the_session() {
    let mut conn = Connection::new(plain_config()).expect("config");
    conn.connected(Instant::now()).expect("connected");
    feed(&mut conn, "421 4.3.2 Service not available\r\n");
    let produced = events(&mut conn);
    assert!(
        produced
            .iter()
            .any(|e| matches!(e, Event::Failed { error, .. } if error.code == "ECONNECTION")),
        "{produced:?}"
    );
    assert!(produced
        .iter()
        .any(|e| matches!(e, Event::CloseTransport | Event::Closed)));
}

/// The engine's own id band and subsystem slot, asserted rather than assumed —
/// `turnloop_net` keys every handle on a thread in ONE map, so an overlap is
/// one subsystem's completion reaching another's socket.
#[test]
fn ids_are_disjoint_from_every_other_subsystem() {
    let common_end = perry_runtime::value::addr_class::COMMON_HANDLE_BAND_END as i64;
    assert!(super::ID_BASE > common_end);
    assert!(super::ID_CEILING < (1i64 << 56));
    assert_ne!(super::SUBSYSTEM, 0, "slot 0 belongs to perry-ext-net");
    assert!((super::SUBSYSTEM as usize) < perry_runtime::turnloop_net::MAX_SUBSYSTEMS);
}

/// Every nodemailer error code this engine can produce must survive the
/// re-interning the runtime's diagnostics registry requires, and an unknown one
/// must degrade to a socket error rather than to something that reads as real.
#[test]
fn error_codes_are_re_interned() {
    for code in [
        "EAUTH",
        "ECONNECTION",
        "EENVELOPE",
        "EMESSAGE",
        "EPROTOCOL",
        "ESOCKET",
    ] {
        assert_eq!(super::intern_code(code), code);
    }
    assert_eq!(super::intern_code("ENOSUCHTHING"), "ESOCKET");
}

fn ready_connection() -> Connection {
    let mut conn = Connection::new(plain_config()).expect("config");
    conn.connected(Instant::now()).expect("connected");
    feed(&mut conn, "220 perry-test ESMTP\r\n");
    drain(&mut conn);
    feed(&mut conn, EHLO_REPLY);
    drain(&mut conn);
    feed(&mut conn, "235 2.7.0 Authentication successful\r\n");
    let _ = events(&mut conn);
    assert_eq!(conn.state(), State::Ready);
    conn
}
