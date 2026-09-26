//! Fetch's destination-port policy must run before transport preparation.
use super::*;

#[test]
fn blocked_port_preflight_rejects_before_proxy_or_tls_preparation() {
    for url in [
        "http://127.0.0.1:22/",
        "https://127.0.0.1:10080/",
        "http://[::1]:00025/",
        // The Fetch standard also blocks zero (Node 26.5.1 currently does not).
        "http://127.0.0.1:0/",
    ] {
        let spec = RequestSpec {
            url: url.to_owned(),
            method: "GET".to_owned(),
            headers: Vec::new(),
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        };
        assert_eq!(
            prepare(&spec).err(),
            Some(Declined::Unsupported(turnloop_http::Error::new(
                "ERR_BAD_PORT",
                "bad port",
            ))),
            "blocked destination must be rejected before transport preparation: {url}"
        );
    }
}

#[test]
fn allowed_ports_and_non_http_schemes_are_not_blocked() {
    for url in [
        "http://example.test/",
        "https://example.test/",
        "http://example.test:80/",
        "https://example.test:443/",
        "http://example.test:6664/",
        "http://example.test:6670/",
        "http://example.test:6678/",
        "http://example.test:6680/",
        "http://example.test:65535/",
        "ftp://example.test:22/",
    ] {
        assert_eq!(port_policy::check(&url::Url::parse(url).unwrap()), Ok(()));
    }
}

static BAD_PORT_COMPLETIONS: AtomicU64 = AtomicU64::new(0);

fn bad_port_done(_: usize, outcome: Outcome) {
    match outcome {
        Outcome::Err(error) => {
            assert_eq!(error.code, "ERR_BAD_PORT");
            assert_eq!(error.message, "bad port");
        }
        Outcome::Ok(_) => panic!("blocked destination received a response"),
    }
    BAD_PORT_COMPLETIONS.fetch_add(1, Ordering::Relaxed);
}

#[test]
fn redirected_destination_is_rejected_before_connection_allocation() {
    let _owner = become_the_owner_for_test();
    for proxy in [
        None,
        Some(url::Url::parse("http://127.0.0.1:8080/").unwrap()),
    ] {
        let spec = RequestSpec {
            url: "http://127.0.0.1:8081/".to_owned(),
            method: "GET".to_owned(),
            headers: Vec::new(),
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        };
        let mut request = tlc::Request::new(&spec.url, &spec.method).unwrap();
        assert!(request
            .redirect(302, Some("http://127.0.0.1:22/"), RedirectMode::Follow, 20)
            .unwrap());
        let before_id = ENGINE.with(|engine| engine.borrow().next_id);
        let before_reused = REUSED.load(Ordering::Relaxed);
        let before_done = BAD_PORT_COMPLETIONS.load(Ordering::Relaxed);
        // Redirects bypass preflight: exercise the connection entry point with
        // an already redirected request, both directly and behind a proxy.
        start_here(
            spec,
            Sink {
                ctx: 0,
                on_head: None,
                on_chunk: None,
                on_done: bad_port_done,
            },
            Prepared { request, proxy },
        )
        .unwrap();
        assert_eq!(
            BAD_PORT_COMPLETIONS.load(Ordering::Relaxed),
            before_done + 1
        );
        assert_eq!(ENGINE.with(|engine| engine.borrow().next_id), before_id);
        assert_eq!(REUSED.load(Ordering::Relaxed), before_reused);
    }
}
