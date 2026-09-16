//! `mongodb` on a turnloop socket (P7).
//!
//! What this replaces, one for one:
//!
//! | before | after |
//! |---|---|
//! | `spawn_blocking` + `Handle::current().block_on` per call — one tokio blocking-pool thread held for the whole round trip | one command on a sans-I/O core, submitted where the FFI call happens |
//! | the `mongodb` crate's connection pool and its background tokio tasks | one `turnloop_mongodb::Connection` driven over P1's `turnloop_net` |
//! | the crate's own timers | the core's deadline, armed as a real turnloop deadline |
//!
//! # Scope: one topology, either transport security
//!
//! This slice migrates a **direct, single-server** connection and nothing else.
//! [`classify_uri`] accepts a `mongodb://host:port/...` URI with no `+srv`, no
//! `replicaSet`, one host, and no compressor; every other URI **declines** and
//! keeps the existing `mongodb` crate path, unchanged, including its SRV/DNS
//! resolution, replica-set topology discovery with background monitors and its
//! own pool. None of that is reimplemented here and none of it is deleted — the
//! declining cases are real configurations that still run.
//!
//! `tls=true`/`ssl=true` is no longer one of them: `turnloop_mongodb` asks its
//! host to perform the upgrade and `perry-db-turnloop` now has a TLS client to
//! give it ([`tls_options`]). The per-connection TLS *keys* still decline —
//! `tlsCAFile`, `tlsInsecure`, `tlsAllowInvalidCertificates` and
//! `tlsCertificateKeyFile` are rejected by `Options::parse`, so they never
//! reach this decision — because this path honours the process TLS environment
//! and has nowhere to put a trust store belonging to one URI.
//!
//! A URI that `turnloop_mongodb::uri::Options` cannot parse also declines,
//! which is deliberate: the legacy path then produces its own
//! `Failed to parse URI: …` rejection, so the error a bad URI gets does not
//! depend on which parser saw it first.
//!
//! # Lazy connection, and why `connect()` still resolves immediately
//!
//! `mongodb::Client::with_options` performs no I/O — the pre-P7
//! `client.connect()` therefore resolved even with no server running, and the
//! first *operation* was what failed. A connection here is opened on the first
//! operation for exactly that reason: opening eagerly would turn
//! `await client.connect()` against a down server from a resolved promise into
//! a rejected one, in existing programs.
//!
//! # Threading and the GC
//!
//! The sink runs on the agent thread from the loop's own completion dispatch,
//! so it may touch the connection table directly. It builds **no JS value**: a
//! result is settled through `JsPromise::resolve_with`, whose closure carries
//! owned Rust data and runs on the main thread during the resolution pump.
//! That is the same #1824 rule the `spawn_blocking` path had to obey.

mod connection;
mod ops;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bson::oid::ObjectId;
use bson::Document;
use perry_db_turnloop::{subsystem, NetCompletion, Registry, TlsClientOptions};
use perry_ffi::{Handle, JsPromise};
use turnloop_mongodb::command::ObjectIdGenerator;
use turnloop_mongodb::uri::Options;

use self::connection::MongoCore;
use self::ops::{Operation, Request};

/// This binding's slot in the runtime's sink registry.
pub(crate) const SUBSYSTEM: u8 = subsystem::MONGODB;

/// The connect timeout the pre-P7 path installed when the URI did not give one
/// (`mongodb://…` with no `connectTimeoutMS`). `turnloop_mongodb`'s own default
/// is 30 s, so without this a dead host would take six times longer to report
/// itself than it does today.
const LEGACY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

thread_local! {
    /// The connection table. Thread-local because a turnloop handle belongs to
    /// the loop that created it — see `perry_db_turnloop`'s module docs.
    static REGISTRY: Registry<MongoCore> = Registry::new(SUBSYSTEM);
    /// A client handle → the driver id of its connection. Absent until the
    /// first operation opens one.
    static OPEN: RefCell<HashMap<Handle, i64>> = RefCell::new(HashMap::new());
    /// The process's `ObjectId` generator, seeded from OS entropy on first use.
    static OBJECT_IDS: RefCell<Option<ObjectIdGenerator>> = const { RefCell::new(None) };
}

/// Where a turnloop client connects, and on what terms.
///
/// Held by the client handle, so the transport decision is made once and the
/// parsed options never have to be re-derived — or re-decided — later.
#[derive(Clone, Debug)]
pub(crate) struct Endpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) options: Options,
}

/// A collection, named by the client whose connection carries it.
///
/// A database and a collection handle are cheap views: on this transport they
/// hold names and the client's handle, never a connection of their own, so
/// `db.collection(x)` costs no socket and every operation on every collection
/// of one client shares that client's single connection.
#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) client: Handle,
    pub(crate) database: String,
    pub(crate) collection: String,
}

// ---------------------------------------------------------------------------
// Transport selection
// ---------------------------------------------------------------------------

/// Whether a client created *now, on this thread* should live on turnloop.
pub(crate) fn enabled() -> bool {
    REGISTRY.with(|reg| reg.enabled(sink))
}

/// Install the sink and report whether the runtime accepted it.
///
/// Separate from [`enabled`] so a test can assert the part that is a property
/// of the build — the completion-layout digest check — without also asserting
/// that the thread it happens to run on owns a loop. `cargo test` puts each
/// test on its own thread and only some of them do.
#[cfg(test)]
pub(crate) fn register_only() -> bool {
    REGISTRY.with(|reg| reg.register(sink))
}

/// Decide a URI's transport. `None` means "keep the legacy path".
pub(crate) fn classify(uri: &str) -> Option<Endpoint> {
    let endpoint = classify_uri(uri)?;
    // Asked last, and asked per client: a `worker_threads` agent has no loop of
    // its own, and the answer is not a property of the URI.
    enabled().then_some(endpoint)
}

/// The URI half of the decision, with no runtime state involved.
///
/// Split out so the accept/decline rules can be tested for what they are — a
/// scope boundary — rather than only in combination with a live sink registry.
///
/// `tls=true` is *inside* that boundary: the driver installs the session the
/// core asks for. Every other `tls*` key is outside it and declines one step
/// earlier, in `Options::parse` — see the module docs for why that is the
/// honest place for it rather than a check here.
pub(crate) fn classify_uri(uri: &str) -> Option<Endpoint> {
    let mut options = Options::parse(uri).ok()?;
    // Each of these is a whole subsystem this slice does not implement.
    if options.srv.is_some() {
        // `mongodb+srv://` needs SRV and TXT resolution before there is an
        // address to connect to.
        return None;
    }
    if options.replica_set.is_some() {
        // A replica set needs topology discovery and a primary election to
        // follow; one socket to one seed is not that.
        return None;
    }
    if options.seeds.len() != 1 {
        // Several hosts means server selection.
        return None;
    }
    if !options.compressors.is_empty() {
        // Compression is deliberately left off (P7 scope); the binding exposes
        // no option for it, so a URI that asks for it declines rather than
        // silently getting an uncompressed connection.
        return None;
    }
    if !entropy_available() {
        // SCRAM's security rests on an unpredictable client nonce that the host
        // supplies. Without an entropy source there is no correct nonce to
        // give, and a fixed one is worse than declining.
        return None;
    }
    if !options.raw.contains_key("connecttimeoutms") {
        options.connect_timeout = LEGACY_CONNECT_TIMEOUT;
    }
    let seed = options.seeds.first()?.clone();
    Some(Endpoint {
        host: seed.host,
        port: seed.port,
        options,
    })
}

// ---------------------------------------------------------------------------
// Host entropy
// ---------------------------------------------------------------------------

/// Read OS entropy.
///
/// `/dev/urandom` rather than a crate: adding a dependency for this would have
/// meant a `Cargo.lock` change, and this is the same source `getrandom` uses on
/// the platforms Perry's database bindings build for. Returns false rather than
/// falling back to anything weaker — every caller treats that as "decline",
/// never as "use a predictable value".
fn os_entropy(buffer: &mut [u8]) -> bool {
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(buffer))
        .is_ok()
}

/// Probe the entropy source once. Cached because [`classify_uri`] asks per
/// client and the answer cannot change within a process.
fn entropy_available() -> bool {
    static STATE: AtomicU8 = AtomicU8::new(0);
    match STATE.load(Ordering::Relaxed) {
        0 => {
            let mut probe = [0u8; 1];
            let available = os_entropy(&mut probe);
            STATE.store(if available { 2 } else { 1 }, Ordering::Relaxed);
            available
        }
        2 => true,
        _ => false,
    }
}

/// A SCRAM client nonce.
///
/// `turnloop_mongodb::auth::Scram` requires at least 16 printable, comma-free
/// bytes and checks it; the mechanism's replay resistance rests on it being
/// unpredictable, so it comes from the OS CSPRNG. Twenty-four characters drawn
/// from a 64-symbol alphabet carry 144 bits.
///
/// **Unverified end to end**: the test server this slice was written against
/// runs without authentication, so no SCRAM exchange has ever been performed
/// over this code.
fn client_nonce() -> Option<String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bytes = [0u8; 24];
    if !os_entropy(&mut bytes) {
        return None;
    }
    Some(
        bytes
            .iter()
            .map(|b| char::from(ALPHABET[usize::from(b & 63)]))
            .collect(),
    )
}

/// The next client-generated `_id`.
///
/// The five random bytes and the counter's starting value are per process and
/// come from the OS, as the ObjectId specification requires; the timestamp is
/// wall-clock seconds, which the sans-I/O generator will not read for itself.
pub(crate) fn next_object_id() -> Option<ObjectId> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as u32;
    OBJECT_IDS.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let mut seed = [0u8; 8];
            if !os_entropy(&mut seed) {
                return None;
            }
            let random = [seed[0], seed[1], seed[2], seed[3], seed[4]];
            let counter = u32::from_be_bytes([0, seed[5], seed[6], seed[7]]);
            *slot = Some(ObjectIdGenerator::new(random, counter));
        }
        slot.as_mut().map(|generator| generator.generate(seconds))
    })
}

// ---------------------------------------------------------------------------
// Connection lifecycle
// ---------------------------------------------------------------------------

extern "C" fn sink(completion: *const NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime borrows one completion for the duration of this call.
    let completion = unsafe { &*completion };
    let id = completion.id;
    let retired = REGISTRY.with(|reg| {
        reg.dispatch(completion);
        !reg.is_live(id)
    });
    if retired {
        OPEN.with(|open| open.borrow_mut().retain(|_, v| *v != id));
    }
}

/// The TLS options the driver installs when the core asks for the upgrade.
///
/// `None` for a plaintext URI, which is what makes an `UpgradeTls` request on
/// such a connection a driver error rather than a silent plaintext
/// continuation.
fn tls_options(endpoint: &Endpoint) -> Option<TlsClientOptions> {
    if !endpoint.options.tls {
        return None;
    }
    // The servername is the seed host, and there is only ever one: a
    // multi-seed URI declines, and an SRV URI — whose resolved hosts differ
    // from the name in the URI — declines before this. The key that would
    // override it, `tlsAllowInvalidHostnames`, is one `Options::parse`
    // refuses, so no configuration in scope wants a name other than the one
    // the client dialled.
    //
    // Everything else comes from the process TLS environment
    // (`NODE_TLS_REJECT_UNAUTHORIZED`, `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`).
    // That is not a gap left for later: the per-connection spellings are
    // exactly the keys that decline, so a URI this path accepts is one whose
    // only answer is the process answer, and the two can never disagree about
    // a single connection.
    //
    // No ALPN: a MongoDB connection carries OP_MSG and nothing else.
    Some(TlsClientOptions::from_node_environment(
        endpoint.host.clone(),
    ))
}

/// Open `client`'s connection if it has none, and return its driver id.
fn open(client: Handle) -> Result<i64, String> {
    if let Some(id) = OPEN.with(|open| open.borrow().get(&client).copied()) {
        if REGISTRY.with(|reg| reg.is_live(id)) {
            return Ok(id);
        }
        OPEN.with(|open| {
            open.borrow_mut().remove(&client);
        });
    }
    let endpoint = crate::turnloop_endpoint(client).ok_or("Invalid client handle")?;
    let nonce = client_nonce().ok_or("No OS entropy for a SCRAM nonce")?;
    // Read before `options` moves into the core: the endpoint carries the
    // `tls` bool that decides this, and the core carries the one that makes it
    // ask, so they are two readings of the same parsed URI.
    let tls = tls_options(&endpoint);
    let core = MongoCore::new(endpoint.options, nonce);
    let id = REGISTRY.with(|reg| {
        reg.connect_with_tls(
            &endpoint.host,
            endpoint.port,
            core,
            client.try_into().unwrap_or(0),
            tls,
        )
    })?;
    OPEN.with(|open| {
        open.borrow_mut().insert(client, id);
    });
    Ok(id)
}

/// Submit one operation on `client`'s connection, answering `promise`.
///
/// Every path settles or parks the promise, including every failure: once a
/// client has been created on this transport there is no falling back, because
/// an earlier operation may already be queued on this connection and running
/// this one through the legacy path would reorder them.
fn submit(client: Handle, promise: JsPromise, built: Result<(Operation, Request), String>) {
    let (operation, request) = match built {
        Ok(built) => built,
        Err(message) => {
            promise.reject_string(&message);
            return;
        }
    };
    let id = match open(client) {
        Ok(id) => id,
        Err(message) => {
            operation.reject(promise, &message);
            return;
        }
    };
    // The promise travels through an `Option` so that a `with_core` which never
    // runs its closure — the entry went away between `open` and here — hands it
    // back instead of dropping it. A dropped `JsPromise` is a promise that never
    // settles, which is the one outcome a caller cannot recover from.
    let mut slot = Some((operation, promise));
    let submitted = REGISTRY.with(|reg| {
        reg.with_core(id, |core| {
            let (operation, promise) = slot.take().expect("the closure runs at most once");
            core.submit(operation, request, promise);
        })
    });
    if submitted.is_none() {
        if let Some((operation, promise)) = slot {
            // `reject` supplies the operation's own prefix, so this message
            // must not repeat it.
            operation.reject(promise, "MongoDB connection is closed");
        }
    }
}

/// `client.connect()` on this transport.
///
/// Resolves immediately: see the module docs on why an eager connect would be a
/// behaviour change rather than a fix.
pub(crate) fn connect(promise: JsPromise) {
    promise.resolve_undefined();
}

// ---------------------------------------------------------------------------
// Operations — one per JS-visible entry point
// ---------------------------------------------------------------------------

pub(crate) fn find_one(target: Target, promise: JsPromise, filter_json: &str) {
    let built = Operation::find_one(&target.database, &target.collection, filter_json);
    submit(target.client, promise, built);
}

pub(crate) fn find(target: Target, promise: JsPromise, filter_json: &str) {
    let built = Operation::find(&target.database, &target.collection, filter_json);
    submit(target.client, promise, built);
}

pub(crate) fn insert_one(target: Target, promise: JsPromise, document: Document) {
    let built = Operation::insert_one(&target.database, &target.collection, document);
    submit(target.client, promise, built);
}

pub(crate) fn insert_many(target: Target, promise: JsPromise, documents: Vec<Document>) {
    let built = Operation::insert_many(&target.database, &target.collection, documents);
    submit(target.client, promise, built);
}

pub(crate) fn update(
    target: Target,
    promise: JsPromise,
    filter_json: &str,
    update_doc: Document,
    many: bool,
) {
    let built = Operation::update(
        &target.database,
        &target.collection,
        filter_json,
        update_doc,
        many,
    );
    submit(target.client, promise, built);
}

pub(crate) fn delete(target: Target, promise: JsPromise, filter_json: &str, many: bool) {
    let built = Operation::delete(&target.database, &target.collection, filter_json, many);
    submit(target.client, promise, built);
}

pub(crate) fn count(target: Target, promise: JsPromise, filter_json: &str) {
    let built = Operation::count(&target.database, &target.collection, filter_json);
    submit(target.client, promise, built);
}

pub(crate) fn list_databases(client: Handle, promise: JsPromise) {
    submit(client, promise, Operation::list_databases());
}

pub(crate) fn list_collections(client: Handle, database: &str, promise: JsPromise) {
    submit(client, promise, Operation::list_collections(database));
}
