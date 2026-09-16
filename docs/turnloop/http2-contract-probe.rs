use turnloop_http::http1::Header;
use turnloop_http::http2::{Connection, Event, Limits, Role};

fn drive(conn: &mut Connection, input: &mut Vec<u8>) -> Result<Vec<String>, String> {
    let mut seen = Vec::new();
    loop {
        let r = conn.receive(input);
        let (consumed, note) = match r {
            Ok(step) => {
                let note = step.event.as_ref().map(|e| match e {
                    Event::Settings => "Settings".to_string(),
                    Event::Headers { stream, end_stream, .. } => format!("Headers s={stream} end={end_stream}"),
                    Event::Data { stream, bytes, end_stream } => format!("Data s={stream} n={} end={end_stream}", bytes.len()),
                    Event::Reset { stream, code } => format!("Reset s={stream} code={code}"),
                    Event::Goaway { last_stream, code } => format!("Goaway last={last_stream} code={code}"),
                    Event::Ping { ack, .. } => format!("Ping ack={ack}"),
                    Event::WindowUpdate { stream } => format!("WindowUpdate s={stream}"),
                });
                (step.consumed, note)
            }
            Err(e) => return Err(format!("{}", e.code)),
        };
        let had = note.is_some();
        if let Some(n) = note { seen.push(n); }
        if consumed > 0 { input.drain(..consumed); }
        if consumed == 0 && !had { break; }
    }
    Ok(seen)
}

fn headers(path: &str) -> Vec<Header> {
    vec![Header::new(":method","POST"),Header::new(":scheme","http"),
         Header::new(":path",path),Header::new(":authority","x")]
}
fn ship(from: &mut Connection) -> Vec<u8> {
    let b = from.output().to_vec();
    let n = from.output().len();
    from.consume_output(n).unwrap();
    b
}

/// `release_before_reset`: whether the server returns the DATA window before
/// resetting the stream (rule 3 of the flow-control policy).
fn run(release_before_reset: bool) -> usize {
    let mut limits = Limits::default();
    limits.streams = 2;                       // the SERVER's table: exactly two slots
    let mut server = Connection::new(Role::Server, limits).unwrap();
    // The client's own table is large, and it is told MAX_CONCURRENT_STREAMS=2
    // by the server — so every stream is fully retired on the client before the
    // next is opened, and only the SERVER's table is under test.
    let mut client = Connection::new(Role::Client, Limits::default()).unwrap();

    let mut b = ship(&mut client);
    drive(&mut server, &mut b).unwrap();
    let mut b = ship(&mut server);
    drive(&mut client, &mut b).unwrap();

    let mut accepted = 0usize;
    for i in 0..6 {
        let path = format!("/{i}");
        let id = match client.open(&headers(&path), false) {
            Ok(id) => id,
            Err(e) => { println!("  [client refused open #{i}: {}]", e.code); break; }
        };
        let _ = client.send_data(id, b"hello-body", true);
        let mut bytes = ship(&mut client);
        match drive(&mut server, &mut bytes) {
            Ok(_) => {}
            Err(code) => { println!("  server refused stream #{i}: CONNECTION ERROR {code}"); break; }
        }
        accepted += 1;
        if release_before_reset {
            // 10 bytes of DATA were charged to this stream.
            server.release_capacity(id, 10).unwrap();
        }
        server.reset(id, 8).unwrap();
        // Ship the RST_STREAM (and any WINDOW_UPDATEs) back so the CLIENT
        // retires its own record too — otherwise the client's peer-limit check
        // would be what refuses, and that is not what we are measuring.
        let mut back = ship(&mut server);
        drive(&mut client, &mut back).unwrap();
    }
    accepted
}

/// Gap 3: is a stream opened after a graceful GOAWAY a stream error (Node) or a
/// connection error?
fn goaway_race() {
    let mut server = Connection::new(Role::Server, Limits::default()).unwrap();
    let mut client = Connection::new(Role::Client, Limits::default()).unwrap();
    let mut b = ship(&mut client);
    drive(&mut server, &mut b).unwrap();
    server.shutdown().unwrap();
    let n = server.output().len();
    server.consume_output(n).unwrap();
    // The client has NOT seen the GOAWAY yet — the unavoidable race — and opens
    // a stream.
    let id = client.open(&headers("/late"), true).unwrap();
    println!("  client opened late stream {id}");
    let mut late = ship(&mut client);
    match drive(&mut server, &mut late) {
        Ok(events) => println!("  server after late stream: {:?}", events),
        Err(code) => println!("  server after late stream: CONNECTION ERROR {code}"),
    }
    let out = server.output();
    if out.len() >= 9 {
        println!("  server emitted frame kind={} (7 = GOAWAY)", out[3]);
    }
}

/// Gap 4: the two independent zero cases of `Step`.
fn step_taxonomy() {
    use turnloop_http::http2::{encode_frame, PREFACE};
    let mut server = Connection::new(Role::Server, Limits::default()).unwrap();
    let step = server.receive(&PREFACE[..5]).unwrap();
    println!(
        "  partial preface: consumed={} event={}",
        step.consumed,
        step.event.is_some()
    );
    let mut cursor: Vec<u8> = PREFACE.to_vec();
    let mut f = Vec::new();
    encode_frame(4, 0, 0, &[], &mut f).unwrap();       // peer SETTINGS
    cursor.extend_from_slice(&f);
    let mut f = Vec::new();
    encode_frame(4, 1, 0, &[], &mut f).unwrap();       // peer SETTINGS ack
    cursor.extend_from_slice(&f);
    loop {
        let step = server.receive(&cursor).unwrap();
        let label = match step.event {
            Some(Event::Settings) => "Some(Settings)",
            Some(_) => "Some(other)",
            None => "None",
        };
        println!("  step: consumed={} event={}", step.consumed, label);
        let c = step.consumed;
        let had = label != "None";
        if c > 0 {
            cursor.drain(..c);
        }
        if c == 0 && !had {
            break;
        }
    }
}

fn main() {
    println!("== gap 1: a reset stream's table slot ==");
    println!("-- server resets WITHOUT releasing capacity --");
    let a = run(false);
    println!("   streams the server accepted: {a}");
    println!("-- server releases capacity, THEN resets --");
    let b = run(true);
    println!("   streams the server accepted: {b}");
    println!("   verdict: without release = {a}, with release = {b} (table size 2, 6 attempted)");
    println!();
    println!("== gap 3: a stream opened after a graceful GOAWAY ==");
    goaway_race();
    println!();
    println!("== gap 4: the two zero cases of Step ==");
    step_taxonomy();
}
