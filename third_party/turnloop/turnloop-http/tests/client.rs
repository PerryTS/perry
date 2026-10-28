use std::time::{Duration, Instant};
use turnloop_http::{
    client::{
        Acquire, Completion, ConnectionId, Deadlines, Http1Connection, Lifecycle, Phase, Pool,
        PoolKey, Protocol, Request, RequestId,
    },
    http1::{BodyLength, Header, Limits},
};

fn key() -> PoolKey {
    PoolKey::new(&Request::new("http://a.test/", "GET").unwrap().url, None)
}

#[test]
fn a_stale_reuse_id_can_be_checked_and_forgotten() {
    let now = Instant::now();
    let key = key();
    let mut pool = Pool::new(1, Duration::from_secs(5));
    let Acquire::Connect(id) = pool.acquire(&key, now) else {
        panic!("first acquire connects")
    };
    assert!(pool.contains(id));
    pool.connected(id, Protocol::Http1, 1).unwrap();
    pool.release(id, true, now).unwrap();
    assert_eq!(pool.acquire(&key, now), Acquire::Reuse(id));
    // The host looks the id up and finds no socket: the pool is told directly.
    assert!(pool.forget(id));
    assert!(!pool.contains(id));
    assert!(!pool.forget(id), "a stale id is reported, not an error");
    assert_eq!(
        pool.release(id, false, now).unwrap_err().code,
        "UND_ERR_CLOSED"
    );
    // The single per-host place is free again, under a new id.
    let Acquire::Connect(next) = pool.acquire(&key, now) else {
        panic!("forget frees the per-host place")
    };
    assert_ne!(next, id);
    assert!(pool.contains(next) && !pool.contains(id));
    // A forgotten idle connection's deadline does not fire later.
    pool.connected(next, Protocol::Http1, 1).unwrap();
    pool.release(next, true, now).unwrap();
    assert_eq!(pool.next_timeout(), Some(now + Duration::from_secs(5)));
    assert!(pool.forget(next));
    assert_eq!(pool.next_timeout(), None);
    assert_eq!(pool.handle_timeout(now + Duration::from_secs(60)), None);
    assert!(!pool.contains(ConnectionId(999)));
    assert!(!pool.forget(ConnectionId(999)));
}

#[test]
fn contains_follows_close_and_idle_expiry() {
    let now = Instant::now();
    let key = key();
    let mut pool = Pool::new(2, Duration::from_secs(1));
    let Acquire::Connect(a) = pool.acquire(&key, now) else {
        panic!()
    };
    let Acquire::Connect(b) = pool.acquire(&key, now) else {
        panic!()
    };
    pool.closed(a).unwrap();
    assert!(!pool.contains(a) && pool.contains(b));
    pool.connected(b, Protocol::Http1, 1).unwrap();
    pool.release(b, true, now).unwrap();
    assert_eq!(pool.handle_timeout(now + Duration::from_secs(1)), Some(b));
    assert!(!pool.contains(b));
}

#[test]
fn one_next_timeout_covers_idle_connections_and_request_lifecycles() {
    let now = Instant::now();
    let at = |s| now + Duration::from_secs(s);
    let mut pool = Pool::new(64, Duration::from_secs(30));
    // 50 origins with one idle connection each: idle deadlines at now + 30s.
    let mut idle = Vec::new();
    for n in 0..50 {
        let url = Request::new(&format!("http://h{n}.test/"), "GET")
            .unwrap()
            .url;
        let Acquire::Connect(id) = pool.acquire(&PoolKey::new(&url, None), now) else {
            panic!()
        };
        pool.connected(id, Protocol::Http1, 1).unwrap();
        pool.release(id, true, now).unwrap();
        idle.push(id);
    }
    assert_eq!(pool.next_timeout(), Some(at(30)));
    // 100 in-flight requests whose lifecycles the host registers.
    let mut requests: Vec<Lifecycle> = (0..100).map(|_| Lifecycle::default()).collect();
    for (n, lifecycle) in requests.iter_mut().enumerate() {
        lifecycle.transition(Phase::Headers, Some(at(40 + n as u64)));
        pool.set_request_deadline(RequestId(n as u64), lifecycle.next_timeout());
    }
    requests[63].transition(Phase::Headers, Some(at(10)));
    pool.set_request_deadline(RequestId(63), requests[63].next_timeout());
    assert_eq!(pool.next_timeout(), Some(at(10)));
    assert_eq!(pool.request_deadline(RequestId(63)), Some(at(10)));
    // Headers arrived: the body phase has no deadline, so the entry goes away.
    requests[63].transition(Phase::Body, None);
    pool.set_request_deadline(RequestId(63), requests[63].next_timeout());
    assert_eq!(pool.next_timeout(), Some(at(30)));
    assert_eq!(pool.handle_request_timeout(at(35)), None);
    // Idle connections expire first, then requests in deadline order.
    let mut closed = Vec::new();
    while let Some(id) = pool.handle_timeout(at(41)) {
        closed.push(id);
    }
    assert_eq!(closed.len(), 50);
    assert!(idle.iter().all(|id| closed.contains(id)));
    assert_eq!(pool.next_timeout(), Some(at(40)));
    let mut fired = Vec::new();
    while let Some(RequestId(n)) = pool.handle_request_timeout(at(41)) {
        let lifecycle = &mut requests[n as usize];
        lifecycle.handle_timeout(at(41));
        let Some(Completion::Error(error)) = lifecycle.poll() else {
            panic!("request {n} should have timed out")
        };
        assert_eq!(error.code, "UND_ERR_HEADERS_TIMEOUT");
        pool.set_request_deadline(RequestId(n), lifecycle.next_timeout());
        fired.push(n);
    }
    assert_eq!(fired, [0, 1]);
    assert_eq!(pool.next_timeout(), Some(at(42)));
}

#[test]
fn http1_connection_deadlines_feed_the_pool() {
    let now = Instant::now();
    let request = Request::new("http://a.test/upload", "POST").unwrap();
    let mut head = request.head(false);
    head.headers.push(Header::new("expect", "100-continue"));
    let mut connection = Http1Connection::new(Limits::default());
    connection
        .start(
            &head,
            BodyLength::Known(4),
            Some(now + Duration::from_secs(10)),
            Some(now + Duration::from_secs(1)),
        )
        .unwrap();
    let mut pool = Pool::new(1, Duration::from_secs(30));
    pool.set_request_deadline(RequestId(1), connection.next_timeout());
    assert_eq!(pool.next_timeout(), Some(now + Duration::from_secs(1)));
    assert_eq!(
        pool.handle_request_timeout(now + Duration::from_secs(1)),
        Some(RequestId(1))
    );
    connection.handle_timeout(now + Duration::from_secs(1));
    assert!(connection.can_send_body());
    pool.set_request_deadline(RequestId(1), connection.next_timeout());
    assert_eq!(pool.next_timeout(), Some(now + Duration::from_secs(10)));
}

#[test]
fn deadlines_fire_in_order_and_ignore_replaced_values() {
    let now = Instant::now();
    let mut deadlines = Deadlines::new();
    assert_eq!(deadlines.next_timeout(), None);
    deadlines.set('a', Some(now + Duration::from_secs(3)));
    deadlines.set('b', Some(now + Duration::from_secs(1)));
    deadlines.set('c', Some(now + Duration::from_secs(2)));
    deadlines.set('b', Some(now + Duration::from_secs(5)));
    deadlines.set('c', None);
    assert_eq!(deadlines.len(), 2);
    assert_eq!(deadlines.get('c'), None);
    assert_eq!(deadlines.next_timeout(), Some(now + Duration::from_secs(3)));
    assert_eq!(deadlines.pop_expired(now + Duration::from_secs(2)), None);
    assert_eq!(
        deadlines.pop_expired(now + Duration::from_secs(9)),
        Some('a')
    );
    assert_eq!(
        deadlines.pop_expired(now + Duration::from_secs(9)),
        Some('b')
    );
    assert_eq!(deadlines.pop_expired(now + Duration::from_secs(9)), None);
    assert!(deadlines.is_empty());
}
