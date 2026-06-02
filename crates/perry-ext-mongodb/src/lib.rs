//! Native bindings for the npm `mongodb` package.
//!
//! Phase 5 step (port mongodb) — first NoSQL-database wrapper to use
//! perry-ffi v0.5.x's full surface (handle registry + `JsPromise` +
//! `spawn_blocking` + `tokio::Handle::current().block_on` async
//! bridge + `json_stringify` for the `_value` variants that take
//! filter/document objects directly from codegen's `NA_F64`
//! coercion). Functionally equivalent to
//! `crates/perry-stdlib/src/mongodb.rs`.
//!
//! # Architecture mirrors perry-stdlib
//!
//! - `MongoClientHandle` lives in two states (pre-connect / connected),
//!   matching the npm `new MongoClient(uri); await client.connect()`
//!   flow plus the older `MongoClient.connect(uri)` combined-factory
//!   path.
//! - All async exports use `spawn_blocking` + `tokio::Handle::current().block_on`
//!   — the `mongodb` crate's API is async-first, but its operations
//!   are I/O-bound, so the blocking pool is the right venue. Same
//!   bridge perry-ext-better-sqlite3 / perry-ext-axios use.
//! - The `*_value` collection-method wrappers (`find_value`,
//!   `insert_one_value`, etc.) bridge codegen's `NA_F64` arg
//!   coercion (which passes the JSValue as f64) onto the
//!   string-taking runtime fns by JSON-stringifying through
//!   `perry_ffi::json_stringify`.
//!
//! # Deferred
//!
//! - BSON `ObjectId` construction across the FFI boundary (returns
//!   the inserted id as a stringified `ObjectId(...)` for now).
//! - Streaming `find` with cursor (today batches the full result via
//!   `try_collect()` and returns one JSON-encoded array string).
//! - Change streams (`watch()`).
//! - Aggregation pipelines beyond the simple `find` filter.

use bson::{doc, Document};
use futures_util::TryStreamExt;
use mongodb::{Client, Collection, Database};
use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, json_stringify, register_handle, spawn_blocking,
    Handle, JsPromise, JsString, JsValue, Promise, StringHeader,
};

/// Helper: extract owned `String` from a runtime `StringHeader`
/// pointer. Returns `None` for null.
unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    perry_ffi::read_string(handle).map(String::from)
}

/// JSON-stringify a NaN-boxed JSValue at the FFI boundary. Used by
/// the `*_value` collection-method wrappers to bridge codegen's
/// `NA_F64` arg coercion (passes the JSValue as f64) to the
/// existing string-taking runtime functions.
///
/// Returns the empty string on stringify failure — the downstream
/// `serde_json::from_str` then surfaces the parse error rather
/// than panicking. Strings that already came in as `STRING_TAG`
/// values pass through verbatim, matching the spec semantic that
/// `JSON.stringify("foo")` → `"\"foo\""`.
fn jsvalue_f64_to_json(value: f64) -> String {
    let v = JsValue::from_bits(value.to_bits());
    json_stringify(v).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Handle types
// ---------------------------------------------------------------------------

/// MongoDB client handle.
///
/// Lives in two states: pre-connect (`pending_uri` holds the URI,
/// `client` is None) and connected (`client` is Some).
/// `new MongoClient(uri)` creates the pre-connect form synchronously;
/// `await client.connect()` flips it to the connected form. The older
/// combined `MongoClient.connect(uri)` factory still returns a
/// fully-connected handle in one step (back-compat with perry-stdlib).
pub struct MongoClientHandle {
    pub client: Option<Client>,
    pub pending_uri: Option<String>,
}

impl MongoClientHandle {
    pub fn new(client: Client) -> Self {
        Self {
            client: Some(client),
            pending_uri: None,
        }
    }

    pub fn pending(uri: String) -> Self {
        Self {
            client: None,
            pending_uri: Some(uri),
        }
    }

    /// Borrow the connected client. Used by query paths that require
    /// a live connection — they bail with `"MongoClient not connected"`
    /// if the user forgot to call `await client.connect()`.
    pub fn client_ref(&self) -> Result<&Client, String> {
        self.client.as_ref().ok_or_else(|| {
            "MongoClient not connected — call await client.connect() first".to_string()
        })
    }
}

/// Database handle wraps a `mongodb::Database`. Cheap to clone —
/// the underlying Client is Arc'd inside the mongodb crate.
pub struct MongoDatabaseHandle {
    pub db: Database,
}

/// Collection handle. `Document` is the BSON document type; we
/// serialize between JSON ↔ BSON at the FFI boundary so user code
/// doesn't see the bson crate at all.
pub struct MongoCollectionHandle {
    pub collection: Collection<Document>,
}

// ---------------------------------------------------------------------------
// Client constructors / connection management
// ---------------------------------------------------------------------------

/// `new MongoClient(uri)` — synchronous constructor matching npm
/// mongodb's API. Stores the URI; the actual `ClientOptions::parse +
/// Client::with_options` work happens inside `.connect()`.
///
/// # Safety
/// `uri_ptr` must be a valid `*const StringHeader` (or null, which
/// produces a handle whose subsequent `.connect()` will fail with a
/// helpful error).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_new(uri_ptr: *const StringHeader) -> Handle {
    let uri = read_str(uri_ptr).unwrap_or_default();
    register_handle(MongoClientHandle::pending(uri))
}

/// `client.connect()` — opens the connection using the URI stored by
/// `js_mongodb_client_new`. Returns Promise<void>. No-op (resolves
/// immediately) if the client was already connected.
///
/// # Safety
/// `client_handle` must be a registered handle; otherwise the
/// promise resolves with undefined (matching perry-stdlib's
/// fail-soft convention).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_connect(client_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    // Take the pending URI on the calling thread (the handle is
    // a runtime handle; `take_pending` is a synchronous local op).
    let pending = if let Some(h) = get_handle_mut::<MongoClientHandle>(client_handle) {
        h.pending_uri.take()
    } else {
        None
    };

    let Some(uri) = pending else {
        // Already connected (or back-compat handle) → resolve immediately.
        promise.resolve_undefined();
        return raw;
    };

    spawn_blocking(move || {
        let result: Result<Client, String> =
            tokio::runtime::Handle::current().block_on(async move {
                let mut opts = mongodb::options::ClientOptions::parse(&uri)
                    .await
                    .map_err(|e| format!("Failed to parse URI: {}", e))?;
                let timeout = std::time::Duration::from_secs(5);
                if opts.connect_timeout.is_none() {
                    opts.connect_timeout = Some(timeout);
                }
                if opts.server_selection_timeout.is_none() {
                    opts.server_selection_timeout = Some(timeout);
                }
                Client::with_options(opts).map_err(|e| format!("Failed to connect: {}", e))
            });

        match result {
            Ok(client) => {
                if let Some(h) = get_handle_mut::<MongoClientHandle>(client_handle) {
                    h.client = Some(client);
                }
                promise.resolve_undefined();
            }
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `MongoClient.connect(uri)` -> Promise<MongoClient> — older
/// combined factory matching the npm mongodb 3.x `connect()` static
/// method. Returns a fully-connected handle in one step.
///
/// # Safety
/// `uri_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_connect(uri_ptr: *const StringHeader) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let uri = match read_str(uri_ptr) {
        Some(u) => u,
        None => {
            promise.reject_string("Invalid URI");
            return raw;
        }
    };

    spawn_blocking(move || {
        let result: Result<Client, String> =
            tokio::runtime::Handle::current().block_on(async move {
                let mut opts = mongodb::options::ClientOptions::parse(&uri)
                    .await
                    .map_err(|e| format!("Failed to parse URI: {}", e))?;
                let timeout = std::time::Duration::from_secs(5);
                if opts.connect_timeout.is_none() {
                    opts.connect_timeout = Some(timeout);
                }
                if opts.server_selection_timeout.is_none() {
                    opts.server_selection_timeout = Some(timeout);
                }
                Client::with_options(opts).map_err(|e| format!("Failed to connect: {}", e))
            });

        match result {
            Ok(client) => {
                let handle = register_handle(MongoClientHandle::new(client));
                // `Handle` is i64; promise resolves with a number so
                // the JS-visible value is the bare handle id matching
                // perry-stdlib's contract.
                promise.resolve_number(handle as f64);
            }
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `client.db(name) -> Database` (synchronous).
///
/// # Safety
/// `name_ptr` must be a `StringHeader` or null (returns -1 sentinel).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_db(
    client_handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    let name = match read_str(name_ptr) {
        Some(n) => n,
        None => return -1,
    };

    // db() requires a connected client. If the user did `new MongoClient(uri)`
    // and skipped `await client.connect()`, return -1 — the same sentinel the
    // null-name path uses. Sub-handle dispatch (db.collection, etc.) will
    // continue to fail-soft with -1 propagation.
    if let Some(client_wrapper) = get_handle::<MongoClientHandle>(client_handle) {
        match client_wrapper.client_ref() {
            Ok(client) => {
                let db = client.database(&name);
                register_handle(MongoDatabaseHandle { db })
            }
            Err(_) => -1,
        }
    } else {
        -1
    }
}

/// `db.collection(name) -> Collection` (synchronous).
///
/// # Safety
/// `name_ptr` must be a `StringHeader` or null (returns -1).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_db_collection(
    db_handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    let name = match read_str(name_ptr) {
        Some(n) => n,
        None => return -1,
    };

    if let Some(db_wrapper) = get_handle::<MongoDatabaseHandle>(db_handle) {
        let collection = db_wrapper.db.collection::<Document>(&name);
        register_handle(MongoCollectionHandle { collection })
    } else {
        -1
    }
}

// ---------------------------------------------------------------------------
// Collection methods (string-path variants — take JSON strings).
// The `_value` variants further down JSON-stringify their f64 args
// then funnel through these.
// ---------------------------------------------------------------------------

/// `collection.findOne(filter) -> Promise<Document | null>`.
/// Resolves with a JSON string (caller does `JSON.parse` themselves)
/// — matches perry-stdlib's pre-existing convention. See the inline
/// comment in mongodb.rs for the rationale.
///
/// # Safety
/// `filter_json_ptr` must be a `StringHeader` or null (treated as `{}`).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find_one(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let filter_json = read_str(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_blocking(move || {
        let result: Result<Option<String>, String> =
            tokio::runtime::Handle::current().block_on(async move {
                if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                    let filter: Document =
                        serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

                    match coll_wrapper.collection.find_one(filter).await {
                        Ok(Some(doc)) => {
                            let json =
                                serde_json::to_string(&doc).unwrap_or_else(|_| "{}".to_string());
                            Ok(Some(json))
                        }
                        Ok(None) => Ok(None),
                        Err(e) => Err(format!("Find failed: {}", e)),
                    }
                } else {
                    Err("Invalid collection handle".to_string())
                }
            });

        match result {
            Ok(Some(json)) => promise.resolve_string(&json),
            Ok(None) => promise.resolve_null(),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.find(filter) -> Promise<Document[]>` — resolves with
/// a JSON-encoded array string. Streaming cursor variant deferred.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let filter_json = read_str(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_blocking(move || {
        let result: Result<String, String> =
            tokio::runtime::Handle::current().block_on(async move {
                if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                    let filter: Document =
                        serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

                    match coll_wrapper.collection.find(filter).await {
                        Ok(cursor) => {
                            let docs: Vec<Document> = cursor
                                .try_collect()
                                .await
                                .map_err(|e| format!("Cursor error: {}", e))?;
                            Ok(serde_json::to_string(&docs).unwrap_or_else(|_| "[]".to_string()))
                        }
                        Err(e) => Err(format!("Find failed: {}", e)),
                    }
                } else {
                    Err("Invalid collection handle".to_string())
                }
            });

        match result {
            Ok(json) => promise.resolve_string(&json),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.insertOne(doc) -> Promise<string>` — resolves with
/// the BSON-stringified `ObjectId` of the inserted doc.
///
/// # Safety
/// `doc_json_ptr` must be a non-null `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_one(
    collection_handle: Handle,
    doc_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let doc_json = match read_str(doc_json_ptr) {
        Some(j) => j,
        None => {
            promise.reject_string("Invalid document");
            return raw;
        }
    };

    spawn_blocking(move || {
        let result: Result<String, String> =
            tokio::runtime::Handle::current().block_on(async move {
                if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                    let doc: Document = serde_json::from_str(&doc_json)
                        .map_err(|e| format!("Invalid JSON: {}", e))?;

                    match coll_wrapper.collection.insert_one(doc).await {
                        Ok(r) => Ok(r.inserted_id.to_string()),
                        Err(e) => Err(format!("Insert failed: {}", e)),
                    }
                } else {
                    Err("Invalid collection handle".to_string())
                }
            });

        match result {
            Ok(id) => promise.resolve_string(&id),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.insertMany(docs) -> Promise<number>` — resolves with
/// the count of inserted ids (matching perry-stdlib's pre-existing
/// convention).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_many(
    collection_handle: Handle,
    docs_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let docs_json = match read_str(docs_json_ptr) {
        Some(j) => j,
        None => {
            promise.reject_string("Invalid documents");
            return raw;
        }
    };

    spawn_blocking(move || {
        let result: Result<u64, String> = tokio::runtime::Handle::current().block_on(async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let docs: Vec<Document> =
                    serde_json::from_str(&docs_json).map_err(|e| format!("Invalid JSON: {}", e))?;

                match coll_wrapper.collection.insert_many(docs).await {
                    Ok(r) => Ok(r.inserted_ids.len() as u64),
                    Err(e) => Err(format!("Insert failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        });

        match result {
            Ok(count) => promise.resolve_number(count as f64),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.updateOne(filter, update) -> Promise<number>` —
/// resolves with `modifiedCount`.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_one(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
    update_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let filter_json = read_str(filter_json_ptr).unwrap_or_else(|| "{}".to_string());
    let update_json = match read_str(update_json_ptr) {
        Some(j) => j,
        None => {
            promise.reject_string("Invalid update");
            return raw;
        }
    };

    spawn_blocking(move || {
        let result: Result<u64, String> = tokio::runtime::Handle::current().block_on(async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let filter: Document =
                    serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});
                let update: Document = serde_json::from_str(&update_json)
                    .map_err(|e| format!("Invalid update JSON: {}", e))?;

                match coll_wrapper.collection.update_one(filter, update).await {
                    Ok(r) => Ok(r.modified_count),
                    Err(e) => Err(format!("Update failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        });

        match result {
            Ok(n) => promise.resolve_number(n as f64),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.updateMany(filter, update) -> Promise<number>` —
/// resolves with `modifiedCount`.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_many(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
    update_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let filter_json = read_str(filter_json_ptr).unwrap_or_else(|| "{}".to_string());
    let update_json = match read_str(update_json_ptr) {
        Some(j) => j,
        None => {
            promise.reject_string("Invalid update");
            return raw;
        }
    };

    spawn_blocking(move || {
        let result: Result<u64, String> = tokio::runtime::Handle::current().block_on(async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let filter: Document =
                    serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});
                let update: Document = serde_json::from_str(&update_json)
                    .map_err(|e| format!("Invalid update JSON: {}", e))?;

                match coll_wrapper.collection.update_many(filter, update).await {
                    Ok(r) => Ok(r.modified_count),
                    Err(e) => Err(format!("Update failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        });

        match result {
            Ok(n) => promise.resolve_number(n as f64),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.deleteOne(filter) -> Promise<number>` — resolves
/// with `deletedCount`.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_one(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let filter_json = read_str(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_blocking(move || {
        let result: Result<u64, String> = tokio::runtime::Handle::current().block_on(async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let filter: Document =
                    serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

                match coll_wrapper.collection.delete_one(filter).await {
                    Ok(r) => Ok(r.deleted_count),
                    Err(e) => Err(format!("Delete failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        });

        match result {
            Ok(n) => promise.resolve_number(n as f64),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.deleteMany(filter) -> Promise<number>` — resolves
/// with `deletedCount`.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_many(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let filter_json = read_str(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_blocking(move || {
        let result: Result<u64, String> = tokio::runtime::Handle::current().block_on(async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let filter: Document =
                    serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

                match coll_wrapper.collection.delete_many(filter).await {
                    Ok(r) => Ok(r.deleted_count),
                    Err(e) => Err(format!("Delete failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        });

        match result {
            Ok(n) => promise.resolve_number(n as f64),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `collection.countDocuments(filter) -> Promise<number>`.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_count(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let filter_json = read_str(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_blocking(move || {
        let result: Result<u64, String> = tokio::runtime::Handle::current().block_on(async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let filter: Document =
                    serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

                match coll_wrapper.collection.count_documents(filter).await {
                    Ok(n) => Ok(n),
                    Err(e) => Err(format!("Count failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        });

        match result {
            Ok(n) => promise.resolve_number(n as f64),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

// ---------------------------------------------------------------------------
// `_value` wrappers — bridge JSValue f64 args to the JSON-string-taking
// runtime fns above. Codegen's NATIVE_MODULE_TABLE rows pass user-supplied
// objects/filters as `NA_F64` (NaN-boxed JSValue), but the existing
// collection-method runtime fns expect a `*const StringHeader` (a
// pre-stringified JSON document). Without these wrappers the dispatch
// table emitted calls that landed inside the f64 bit pattern's bytes
// as if they were a StringHeader — every caller saw `"Invalid document"`
// because `read_str` read garbage. Same fix-shape as the v0.5.270
// ioredis row-name correction.
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_one_value(
    handle: Handle,
    doc_value: f64,
) -> *mut Promise {
    let json = jsvalue_f64_to_json(doc_value);
    let str_ptr = alloc_string(&json).as_raw();
    js_mongodb_collection_insert_one(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_many_value(
    handle: Handle,
    docs_value: f64,
) -> *mut Promise {
    let json = jsvalue_f64_to_json(docs_value);
    let str_ptr = alloc_string(&json).as_raw();
    js_mongodb_collection_insert_many(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_f64_to_json(filter_value);
    let str_ptr = alloc_string(&json).as_raw();
    js_mongodb_collection_find(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find_one_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_f64_to_json(filter_value);
    let str_ptr = alloc_string(&json).as_raw();
    js_mongodb_collection_find_one(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_one_value(
    handle: Handle,
    filter_value: f64,
    update_value: f64,
) -> *mut Promise {
    let filter_json = jsvalue_f64_to_json(filter_value);
    let update_json = jsvalue_f64_to_json(update_value);
    let filter_ptr = alloc_string(&filter_json).as_raw();
    let update_ptr = alloc_string(&update_json).as_raw();
    js_mongodb_collection_update_one(handle, filter_ptr, update_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_many_value(
    handle: Handle,
    filter_value: f64,
    update_value: f64,
) -> *mut Promise {
    let filter_json = jsvalue_f64_to_json(filter_value);
    let update_json = jsvalue_f64_to_json(update_value);
    let filter_ptr = alloc_string(&filter_json).as_raw();
    let update_ptr = alloc_string(&update_json).as_raw();
    js_mongodb_collection_update_many(handle, filter_ptr, update_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_one_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_f64_to_json(filter_value);
    let str_ptr = alloc_string(&json).as_raw();
    js_mongodb_collection_delete_one(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_many_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_f64_to_json(filter_value);
    let str_ptr = alloc_string(&json).as_raw();
    js_mongodb_collection_delete_many(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_count_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_f64_to_json(filter_value);
    let str_ptr = alloc_string(&json).as_raw();
    js_mongodb_collection_count(handle, str_ptr)
}

// ---------------------------------------------------------------------------
// Client-level admin operations
// ---------------------------------------------------------------------------

/// `client.close() -> Promise<void>` — no-op (mongodb's Rust driver
/// manages its connection pool automatically).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_close(_client_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    promise.resolve_undefined();
    raw
}

/// `client.listDatabases() -> Promise<string>` — JSON-encoded
/// array of database names.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_list_databases(client_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let result: Result<String, String> =
            tokio::runtime::Handle::current().block_on(async move {
                if let Some(client_wrapper) = get_handle::<MongoClientHandle>(client_handle) {
                    let client = client_wrapper.client_ref()?;
                    match client.list_database_names().await {
                        Ok(names) => {
                            Ok(serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string()))
                        }
                        Err(e) => Err(format!("List databases failed: {}", e)),
                    }
                } else {
                    Err("Invalid client handle".to_string())
                }
            });

        match result {
            Ok(json) => promise.resolve_string(&json),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

/// `db.listCollections() -> Promise<string>` — JSON-encoded array
/// of collection names.
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_db_list_collections(db_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let result: Result<String, String> =
            tokio::runtime::Handle::current().block_on(async move {
                if let Some(db_wrapper) = get_handle::<MongoDatabaseHandle>(db_handle) {
                    match db_wrapper.db.list_collection_names().await {
                        Ok(names) => {
                            Ok(serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string()))
                        }
                        Err(e) => Err(format!("List collections failed: {}", e)),
                    }
                } else {
                    Err("Invalid database handle".to_string())
                }
            });

        match result {
            Ok(json) => promise.resolve_string(&json),
            Err(msg) => promise.reject_string(&msg),
        }
    });

    raw
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a localhost mongodb URI and verify the option round-
    /// trip surface. No network — `parse` is a sync URI parser only;
    /// it doesn't open any sockets.
    #[test]
    fn client_uri_parses() {
        // `parse` is async in mongodb 3.x but doesn't actually do I/O
        // for a well-formed URI string — it just decodes connection-
        // string params. We use a small tokio runtime to drive it.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let opts = rt.block_on(async {
            mongodb::options::ClientOptions::parse("mongodb://localhost:27017").await
        });
        assert!(opts.is_ok(), "URI should parse: {:?}", opts.err());
    }

    /// Round-trip a small JSON document through serde_json into BSON
    /// `Document` and verify the field count + value matches. Catches
    /// breakage if the bson crate's serde feature ever drifts away
    /// from JSON-compatibility.
    #[test]
    fn bson_document_round_trip() {
        let json = r#"{"a":1,"b":"hello","c":true}"#;
        let doc: Document = serde_json::from_str(json).expect("parse json into bson Document");
        assert_eq!(doc.len(), 3);
        assert_eq!(doc.get_i32("a").unwrap(), 1);
        assert_eq!(doc.get_str("b").unwrap(), "hello");
        assert_eq!(doc.get_bool("c").unwrap(), true);

        // Re-stringify and verify the field set is preserved (object
        // key order in bson Document is insertion order).
        let out = serde_json::to_string(&doc).expect("re-stringify");
        // `a` shows up first.
        assert!(out.contains("\"a\""));
        assert!(out.contains("\"b\""));
        assert!(out.contains("\"c\""));
    }

    /// Verify that a registered collection handle returns -1 from
    /// `js_mongodb_db_collection` when the db handle doesn't exist.
    #[test]
    fn collection_lookup_invalid_db_handle() {
        // 999_999 is far beyond any legitimately-allocated handle
        // (the registry counter starts at 1 and increments).
        let result = unsafe {
            // Allocate the name through perry-ffi so we exercise the
            // string-from-header read path inline. Then call the FFI
            // entry directly (no registered db handle — will return -1).
            let name = alloc_string("users");
            js_mongodb_db_collection(999_999, name.as_raw())
        };
        assert_eq!(result, -1);
    }

    /// Construct a pre-connect MongoClientHandle, register it, and
    /// confirm `client_ref()` returns the `not connected` error
    /// before `.connect()` runs. No network involvement.
    #[test]
    fn pre_connect_client_ref_errors() {
        let h = MongoClientHandle::pending("mongodb://localhost:27017".to_string());
        let err = h.client_ref().unwrap_err();
        assert!(
            err.contains("not connected"),
            "expected `not connected` in error: {}",
            err
        );

        // Registering and looking up by handle id should round-trip.
        let id = register_handle(h);
        assert!(id > 0);
        let stored = get_handle::<MongoClientHandle>(id).expect("registered");
        assert!(stored.client.is_none());
        assert_eq!(
            stored.pending_uri.as_deref(),
            Some("mongodb://localhost:27017")
        );
    }
}
