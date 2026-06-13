//! MongoDB module
//!
//! Native implementation of the 'mongodb' npm package.
//! Provides MongoDB client functionality.

use crate::common::{
    get_handle, register_handle, spawn_for_promise, spawn_for_promise_deferred, Handle,
};
use bson::{doc, Document};
use mongodb::{Client, Collection, Database};
use perry_runtime::json::js_json_stringify;
use perry_runtime::{
    js_object_alloc, js_object_set_field, js_promise_new, js_string_from_bytes, JSValue,
    ObjectHeader, Promise, StringHeader,
};

/// JSON-stringify a NaN-boxed JSValue at the FFI boundary. Used by the
/// `*_value` collection-method wrappers below to bridge codegen's
/// `NA_F64` arg coercion (passes the JSValue as f64) to the existing
/// runtime functions (which expect a JSON string pointer).
///
/// Returns the empty string on null/undefined or stringify failure —
/// the downstream `serde_json::from_str` then surfaces the parse error
/// rather than panicking on a NULL StringHeader read. Strings that
/// already came in as STRING_TAG values pass through verbatim, matching
/// the spec semantic that `JSON.stringify("foo")` → `"\"foo\""`.
unsafe fn jsvalue_to_json_string(value: f64) -> String {
    let str_ptr = js_json_stringify(value, 0);
    if str_ptr.is_null() {
        return String::new();
    }
    string_from_header(str_ptr).unwrap_or_default()
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// MongoDB client handle.
///
/// Lives in two states like PgConnectionHandle: pre-connect (`pending_uri`
/// holds the URI, `client` is None) and connected (`client` is Some).
/// `new MongoClient(uri)` creates the pre-connect form synchronously;
/// `await client.connect()` flips it to the connected form. The older
/// combined `js_mongodb_connect` factory still returns a fully-connected
/// handle in one step (back-compat).
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

    /// Borrow the connected client. Use for query paths that require a
    /// live connection — they bail with `"Client not connected"` if the
    /// user forgot to call `await client.connect()`.
    pub fn client_ref(&self) -> Result<&Client, String> {
        self.client.as_ref().ok_or_else(|| {
            "MongoClient not connected — call await client.connect() first".to_string()
        })
    }
}

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
    let uri = string_from_header(uri_ptr).unwrap_or_default();
    register_handle(MongoClientHandle::pending(uri))
}

/// `client.connect()` — opens the connection using the URI stored by
/// `js_mongodb_client_new`. Returns Promise<void>. No-op (resolves
/// immediately) if the client was already connected (e.g. it came from
/// the older `js_mongodb_connect` combined factory).
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_connect(client_handle: Handle) -> *mut Promise {
    use crate::common::get_handle_mut;

    let promise = js_promise_new();

    let pending = if let Some(h) = get_handle_mut::<MongoClientHandle>(client_handle) {
        h.pending_uri.take()
    } else {
        None
    };

    let Some(uri) = pending else {
        // Already connected (or back-compat handle) → resolve immediately.
        spawn_for_promise(promise as *mut u8, async move {
            Ok(JSValue::undefined().bits())
        });
        return promise;
    };

    spawn_for_promise(promise as *mut u8, async move {
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
        let client = Client::with_options(opts).map_err(|e| format!("Failed to connect: {}", e))?;

        if let Some(h) = get_handle_mut::<MongoClientHandle>(client_handle) {
            h.client = Some(client);
        }
        Ok(JSValue::undefined().bits())
    });

    promise
}

/// MongoDB database handle
pub struct MongoDatabaseHandle {
    pub db: Database,
}

/// MongoDB collection handle
pub struct MongoCollectionHandle {
    pub collection: Collection<Document>,
}

/// Convert BSON Document to JSValue object
#[allow(dead_code)]
unsafe fn bson_to_jsvalue(doc: &Document) -> *mut ObjectHeader {
    let field_count = doc.len() as u32;
    let obj = js_object_alloc(0, field_count);

    let mut idx = 0u32;
    for (_key, value) in doc.iter() {
        let js_val = match value {
            bson::Bson::Null => JSValue::null(),
            bson::Bson::Boolean(b) => JSValue::bool(*b),
            bson::Bson::Int32(n) => JSValue::int32(*n),
            bson::Bson::Int64(n) => JSValue::number(*n as f64),
            bson::Bson::Double(n) => JSValue::number(*n),
            bson::Bson::String(s) => {
                let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                JSValue::string_ptr(ptr)
            }
            bson::Bson::ObjectId(oid) => {
                let s = oid.to_hex();
                let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                JSValue::string_ptr(ptr)
            }
            bson::Bson::Document(nested) => {
                let nested_obj = bson_to_jsvalue(nested);
                JSValue::object_ptr(nested_obj as *mut u8)
            }
            bson::Bson::Array(arr) => {
                // Simplified array handling
                let arr_obj = js_object_alloc(0, arr.len() as u32);
                JSValue::object_ptr(arr_obj as *mut u8)
            }
            _ => JSValue::null(),
        };
        js_object_set_field(obj, idx, js_val);
        idx += 1;
    }

    obj
}

/// MongoClient.connect(uri) -> Promise<MongoClient>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_connect(uri_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let uri = match string_from_header(uri_ptr) {
        Some(u) => u,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid URI".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        let mut opts = mongodb::options::ClientOptions::parse(&uri)
            .await
            .map_err(|e| format!("Failed to parse URI: {}", e))?;
        // Set reasonable timeouts so connect doesn't hang forever
        let timeout = std::time::Duration::from_secs(5);
        if opts.connect_timeout.is_none() {
            opts.connect_timeout = Some(timeout);
        }
        if opts.server_selection_timeout.is_none() {
            opts.server_selection_timeout = Some(timeout);
        }
        let client = Client::with_options(opts).map_err(|e| format!("Failed to connect: {}", e))?;

        let handle = register_handle(MongoClientHandle::new(client));
        Ok(handle as u64)
    });

    promise
}

/// client.db(name) -> Database
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_db(
    client_handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    let name = match string_from_header(name_ptr) {
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

/// db.collection(name) -> Collection
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_db_collection(
    db_handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    let name = match string_from_header(name_ptr) {
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

/// collection.findOne(filter) -> Promise<Document | null>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find_one(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let filter_json = string_from_header(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    // Use deferred to avoid allocating JSValues on worker threads.
    // The async block returns Option<String> (raw Rust data),
    // and the converter creates the JSValue string on the main thread.
    spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let filter: Document =
                    serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

                match coll_wrapper.collection.find_one(filter).await {
                    Ok(Some(doc)) => {
                        let json = serde_json::to_string(&doc).unwrap_or_else(|_| "{}".to_string());
                        Ok(Some(json))
                    }
                    Ok(None) => Ok(None),
                    Err(e) => Err(format!("Find failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        },
        |result: Option<String>| {
            // #4917: resolve a real document object (`doc.field` works), not
            // a JSON string. This converter runs on the main thread, so
            // parsing may allocate JSValues. The JSON comes straight from
            // serde so it always parses; on the off chance it doesn't, fall
            // back to the raw string rather than throwing from inside the
            // resolution pump. BSON-specific types surface in their relaxed
            // extended-JSON shape (e.g. `_id.$oid`).
            match result {
                Some(json) => {
                    let ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
                    match unsafe { perry_runtime::json::js_json_parse_result(ptr) } {
                        Ok(value) => value.bits(),
                        Err(_) => JSValue::string_ptr(ptr).bits(),
                    }
                }
                None => JSValue::null().bits(),
            }
        },
    );

    promise
}

/// collection.find(filter) -> Promise<Document[]>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let filter_json = string_from_header(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    // Use deferred to avoid allocating JSValues on worker threads.
    // The async block returns the JSON string (raw Rust data),
    // and the converter creates the JSValue string on the main thread.
    spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use futures_util::TryStreamExt;

            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let filter: Document =
                    serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

                match coll_wrapper.collection.find(filter).await {
                    Ok(cursor) => {
                        let docs: Vec<Document> = cursor
                            .try_collect()
                            .await
                            .map_err(|e| format!("Cursor error: {}", e))?;

                        let json =
                            serde_json::to_string(&docs).unwrap_or_else(|_| "[]".to_string());
                        Ok(json)
                    }
                    Err(e) => Err(format!("Find failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        },
        |json: String| {
            let ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
            JSValue::string_ptr(ptr).bits()
        },
    );

    promise
}

/// collection.insertOne(doc) -> Promise<InsertOneResult>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_one(
    collection_handle: Handle,
    doc_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let doc_json = match string_from_header(doc_json_ptr) {
        Some(j) => j,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid document".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
                let doc: Document =
                    serde_json::from_str(&doc_json).map_err(|e| format!("Invalid JSON: {}", e))?;

                match coll_wrapper.collection.insert_one(doc).await {
                    Ok(result) => Ok(result.inserted_id.to_string()),
                    Err(e) => Err(format!("Insert failed: {}", e)),
                }
            } else {
                Err("Invalid collection handle".to_string())
            }
        },
        |id: String| {
            let ptr = js_string_from_bytes(id.as_ptr(), id.len() as u32);
            JSValue::string_ptr(ptr).bits()
        },
    );

    promise
}

/// collection.insertMany(docs) -> Promise<InsertManyResult>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_many(
    collection_handle: Handle,
    docs_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let docs_json = match string_from_header(docs_json_ptr) {
        Some(j) => j,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid documents".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
            let docs: Vec<Document> =
                serde_json::from_str(&docs_json).map_err(|e| format!("Invalid JSON: {}", e))?;

            match coll_wrapper.collection.insert_many(docs).await {
                Ok(result) => {
                    let count = result.inserted_ids.len();
                    Ok(count as u64)
                }
                Err(e) => Err(format!("Insert failed: {}", e)),
            }
        } else {
            Err("Invalid collection handle".to_string())
        }
    });

    promise
}

/// collection.updateOne(filter, update) -> Promise<UpdateResult>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_one(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
    update_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let filter_json = string_from_header(filter_json_ptr).unwrap_or_else(|| "{}".to_string());
    let update_json = match string_from_header(update_json_ptr) {
        Some(j) => j,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid update".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
            let filter: Document = serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});
            let update: Document = serde_json::from_str(&update_json)
                .map_err(|e| format!("Invalid update JSON: {}", e))?;

            match coll_wrapper.collection.update_one(filter, update).await {
                Ok(result) => Ok(result.modified_count),
                Err(e) => Err(format!("Update failed: {}", e)),
            }
        } else {
            Err("Invalid collection handle".to_string())
        }
    });

    promise
}

/// collection.updateMany(filter, update) -> Promise<UpdateResult>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_many(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
    update_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let filter_json = string_from_header(filter_json_ptr).unwrap_or_else(|| "{}".to_string());
    let update_json = match string_from_header(update_json_ptr) {
        Some(j) => j,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid update".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
            let filter: Document = serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});
            let update: Document = serde_json::from_str(&update_json)
                .map_err(|e| format!("Invalid update JSON: {}", e))?;

            match coll_wrapper.collection.update_many(filter, update).await {
                Ok(result) => Ok(result.modified_count),
                Err(e) => Err(format!("Update failed: {}", e)),
            }
        } else {
            Err("Invalid collection handle".to_string())
        }
    });

    promise
}

/// collection.deleteOne(filter) -> Promise<DeleteResult>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_one(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let filter_json = string_from_header(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
            let filter: Document = serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

            match coll_wrapper.collection.delete_one(filter).await {
                Ok(result) => Ok(result.deleted_count),
                Err(e) => Err(format!("Delete failed: {}", e)),
            }
        } else {
            Err("Invalid collection handle".to_string())
        }
    });

    promise
}

/// collection.deleteMany(filter) -> Promise<DeleteResult>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_many(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let filter_json = string_from_header(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
            let filter: Document = serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

            match coll_wrapper.collection.delete_many(filter).await {
                Ok(result) => Ok(result.deleted_count),
                Err(e) => Err(format!("Delete failed: {}", e)),
            }
        } else {
            Err("Invalid collection handle".to_string())
        }
    });

    promise
}

/// collection.countDocuments(filter) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_count(
    collection_handle: Handle,
    filter_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let filter_json = string_from_header(filter_json_ptr).unwrap_or_else(|| "{}".to_string());

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(coll_wrapper) = get_handle::<MongoCollectionHandle>(collection_handle) {
            let filter: Document = serde_json::from_str(&filter_json).unwrap_or_else(|_| doc! {});

            match coll_wrapper.collection.count_documents(filter).await {
                Ok(count) => Ok(count),
                Err(e) => Err(format!("Count failed: {}", e)),
            }
        } else {
            Err("Invalid collection handle".to_string())
        }
    });

    promise
}

// ---------------------------------------------------------------------------
// `_value` wrappers — bridge JSValue f64 args to the JSON-string-taking
// runtime fns above. Codegen's NATIVE_MODULE_TABLE rows pass user-supplied
// objects/filters as `NA_F64` (NaN-boxed JSValue), but the existing
// collection-method runtime fns expect a `*const StringHeader` (a
// pre-stringified JSON document). Pre-#187 follow-up these wrappers
// didn't exist and the dispatch table emitted calls that landed inside
// the f64 bit pattern's bytes as if they were a StringHeader — every
// caller saw `"Invalid document"` because string_from_header read garbage.
// Same fix-shape as the v0.5.270 ioredis row-name correction.

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_one_value(
    handle: Handle,
    doc_value: f64,
) -> *mut Promise {
    let json = jsvalue_to_json_string(doc_value);
    let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    js_mongodb_collection_insert_one(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_insert_many_value(
    handle: Handle,
    docs_value: f64,
) -> *mut Promise {
    let json = jsvalue_to_json_string(docs_value);
    let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    js_mongodb_collection_insert_many(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_to_json_string(filter_value);
    let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    js_mongodb_collection_find(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_find_one_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_to_json_string(filter_value);
    let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    js_mongodb_collection_find_one(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_one_value(
    handle: Handle,
    filter_value: f64,
    update_value: f64,
) -> *mut Promise {
    let filter_json = jsvalue_to_json_string(filter_value);
    let update_json = jsvalue_to_json_string(update_value);
    let filter_ptr = js_string_from_bytes(filter_json.as_ptr(), filter_json.len() as u32);
    let update_ptr = js_string_from_bytes(update_json.as_ptr(), update_json.len() as u32);
    js_mongodb_collection_update_one(handle, filter_ptr, update_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_update_many_value(
    handle: Handle,
    filter_value: f64,
    update_value: f64,
) -> *mut Promise {
    let filter_json = jsvalue_to_json_string(filter_value);
    let update_json = jsvalue_to_json_string(update_value);
    let filter_ptr = js_string_from_bytes(filter_json.as_ptr(), filter_json.len() as u32);
    let update_ptr = js_string_from_bytes(update_json.as_ptr(), update_json.len() as u32);
    js_mongodb_collection_update_many(handle, filter_ptr, update_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_one_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_to_json_string(filter_value);
    let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    js_mongodb_collection_delete_one(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_delete_many_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_to_json_string(filter_value);
    let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    js_mongodb_collection_delete_many(handle, str_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_mongodb_collection_count_value(
    handle: Handle,
    filter_value: f64,
) -> *mut Promise {
    let json = jsvalue_to_json_string(filter_value);
    let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    js_mongodb_collection_count(handle, str_ptr)
}

/// client.close() -> Promise<void>
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_close(_client_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    spawn_for_promise(promise as *mut u8, async move {
        // MongoDB client doesn't need explicit close in Rust driver
        // The connection pool is managed automatically
        Ok(JSValue::undefined().bits())
    });

    promise
}

/// client.listDatabases() -> Promise<string> (JSON array of database names)
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_client_list_databases(client_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            if let Some(client_wrapper) = get_handle::<MongoClientHandle>(client_handle) {
                let client = client_wrapper.client_ref()?;
                match client.list_database_names().await {
                    Ok(names) => {
                        let json =
                            serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string());
                        Ok(json)
                    }
                    Err(e) => Err(format!("List databases failed: {}", e)),
                }
            } else {
                Err("Invalid client handle".to_string())
            }
        },
        |json: String| {
            let ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
            JSValue::string_ptr(ptr).bits()
        },
    );

    promise
}

/// db.listCollections() -> Promise<string> (JSON array of collection names)
#[no_mangle]
pub unsafe extern "C" fn js_mongodb_db_list_collections(db_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            if let Some(db_wrapper) = get_handle::<MongoDatabaseHandle>(db_handle) {
                match db_wrapper.db.list_collection_names().await {
                    Ok(names) => {
                        let json =
                            serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string());
                        Ok(json)
                    }
                    Err(e) => Err(format!("List collections failed: {}", e)),
                }
            } else {
                Err("Invalid database handle".to_string())
            }
        },
        |json: String| {
            let ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
            JSValue::string_ptr(ptr).bits()
        },
    );

    promise
}
