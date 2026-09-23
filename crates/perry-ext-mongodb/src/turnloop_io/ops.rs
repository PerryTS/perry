//! One MongoDB operation: what goes on the wire, and what its reply becomes.
//!
//! Nothing here touches a socket, a promise or the connection state machine.
//! It exists as its own module for one reason: every value that leaves a reply
//! must be **owned** before [`super::connection`] releases the receive buffer, and
//! keeping the conversion in one place is what makes that auditable. Every
//! `interpret` arm therefore ends in a `String` or an `f64`, never in a
//! `&RawDocument` borrowed from the core.
//!
//! # Why the results are JSON strings
//!
//! The pre-P7 binding resolved `find`/`findOne` with a JSON string that the JS
//! side parses itself, and resolved counts with numbers. That is the contract
//! perry-stdlib's TypeScript surface is written against, so it is reproduced
//! exactly — including the encoder: both paths run `serde_json::to_string` over
//! a `bson::Document`, so relaxed Extended JSON (`{"$oid": …}`, `{"$date": …}`)
//! comes out byte-for-byte the same on either transport.

use bson::raw::{RawDocument, RawDocumentBuf};
use bson::{Bson, Document};
use perry_ffi::{alloc_string, JsPromise, JsValue};
use turnloop_mongodb::command::{Command, CursorBatch, WriteModel, WriteResult};
use turnloop_mongodb::Error;

/// The bytes of one round trip.
///
/// A single JS-visible operation can need several: a `find` whose result does
/// not fit one batch issues `getMore` until the cursor is exhausted, because
/// the pre-P7 binding's `try_collect()` returned the whole result set and a
/// caller that got only the first 101 documents would be silently wrong.
pub(crate) enum Request {
    /// A command carried entirely in the OP_MSG body.
    Body(RawDocumentBuf),
    /// A write command plus its OP_MSG document sequence.
    WithSequence {
        body: RawDocumentBuf,
        /// `documents`, `updates` or `deletes` — the sequence identifier the
        /// server expects for this command.
        name: &'static str,
        docs: Vec<RawDocumentBuf>,
    },
}

/// What this operation does with a reply, plus whatever it has accumulated.
enum Kind {
    FindOne,
    Find {
        rows: Vec<Document>,
    },
    /// `insertOne` resolves the inserted id, which the *client* chose when the
    /// document had no `_id`, so it is known before the reply arrives.
    InsertOne {
        id: String,
    },
    InsertMany,
    /// `updateOne`/`updateMany` resolve `nModified`.
    UpdateCount,
    /// `deleteOne`/`deleteMany` resolve `n`.
    DeleteCount,
    Count,
    ListDatabases,
    ListCollections {
        names: Vec<String>,
    },
}

/// What the caller's promise is settled with. Owned data only: it is built in
/// the sink and consumed on the main thread.
pub(crate) enum Settlement {
    Json(String),
    Null,
    Number(f64),
}

/// The result of feeding one reply to an operation.
pub(crate) enum Step {
    /// Finished — settle the promise.
    Settle(Settlement),
    /// One more round trip before it can settle.
    More(Request),
}

/// One in-flight JS operation.
pub(crate) struct Operation {
    /// The prefix this operation's rejections carry, e.g. `Find failed`.
    ///
    /// These strings are the pre-P7 binding's, copied verbatim. A program that
    /// matches on `err.message` — and the perry-stdlib TypeScript surface does
    /// not, but user code may — must not be able to tell which transport ran.
    label: &'static str,
    kind: Kind,
    database: String,
    collection: String,
}

impl Operation {
    /// `collection.findOne(filter)`.
    ///
    /// `find` with `limit: 1, singleBatch: true`, which is what makes the
    /// server answer with an already-exhausted cursor (`id: 0`): there is no
    /// cursor left to kill, so this operation is always a single round trip.
    pub(crate) fn find_one(
        database: &str,
        collection: &str,
        filter_json: &str,
    ) -> Result<(Self, Request), String> {
        let filter = raw_filter(filter_json)?;
        let mut command = Command::new();
        command
            .find_one(database, collection, &filter)
            .map_err(|e| format!("Find failed: {}", e))?;
        Ok((
            Self::new("Find failed", Kind::FindOne, database, collection),
            Request::Body(command.raw().to_owned()),
        ))
    }

    /// `collection.find(filter)` — the whole result set, as one JSON array.
    pub(crate) fn find(
        database: &str,
        collection: &str,
        filter_json: &str,
    ) -> Result<(Self, Request), String> {
        let filter = raw_filter(filter_json)?;
        let mut command = Command::new();
        command
            .find(database, collection, &filter, None)
            .map_err(|e| format!("Find failed: {}", e))?;
        Ok((
            Self::new(
                "Find failed",
                Kind::Find { rows: Vec::new() },
                database,
                collection,
            ),
            Request::Body(command.raw().to_owned()),
        ))
    }

    /// `collection.insertOne(document)`.
    ///
    /// `id` is the `_id` the document will carry: the caller's own when it
    /// supplied one, otherwise a client-generated `ObjectId`. The pre-P7 path
    /// got this from the `mongodb` crate, which generates and **prepends**
    /// `_id` the same way; `insert_document` reproduces both halves, because
    /// the field order is observable on the next `find`.
    pub(crate) fn insert_one(
        database: &str,
        collection: &str,
        document: Document,
    ) -> Result<(Self, Request), String> {
        let (document, id) = insert_document(document)?;
        let raw =
            RawDocumentBuf::try_from(&document).map_err(|e| format!("Insert failed: {}", e))?;
        let request = write_request("insert", "documents", database, collection, vec![raw])?;
        Ok((
            Self::new(
                "Insert failed",
                Kind::InsertOne { id: id.to_string() },
                database,
                collection,
            ),
            request,
        ))
    }

    /// `collection.insertMany(documents)` — resolves the inserted count.
    pub(crate) fn insert_many(
        database: &str,
        collection: &str,
        documents: Vec<Document>,
    ) -> Result<(Self, Request), String> {
        let mut raws = Vec::with_capacity(documents.len());
        for document in documents {
            let (document, _) = insert_document(document)?;
            raws.push(
                RawDocumentBuf::try_from(&document).map_err(|e| format!("Insert failed: {}", e))?,
            );
        }
        let request = write_request("insert", "documents", database, collection, raws)?;
        Ok((
            Self::new("Insert failed", Kind::InsertMany, database, collection),
            request,
        ))
    }

    /// `collection.updateOne` / `updateMany` — resolves `modifiedCount`.
    pub(crate) fn update(
        database: &str,
        collection: &str,
        filter_json: &str,
        update_doc: Document,
        many: bool,
    ) -> Result<(Self, Request), String> {
        let filter = raw_filter(filter_json)?;
        let update =
            RawDocumentBuf::try_from(&update_doc).map_err(|e| format!("Update failed: {}", e))?;
        let mut model = WriteModel::default();
        let entry = model
            // `upsert: false` is the npm default and the `mongodb` crate's
            // default; neither this binding's JS surface nor the pre-P7 path
            // ever exposed a way to change it.
            .update(&filter, &update, many, false, None)
            .map_err(|e| format!("Update failed: {}", e))?
            .to_owned();
        let request = write_request("update", "updates", database, collection, vec![entry])?;
        Ok((
            Self::new("Update failed", Kind::UpdateCount, database, collection),
            request,
        ))
    }

    /// `collection.deleteOne` / `deleteMany` — resolves `deletedCount`.
    pub(crate) fn delete(
        database: &str,
        collection: &str,
        filter_json: &str,
        many: bool,
    ) -> Result<(Self, Request), String> {
        let filter = raw_filter(filter_json)?;
        let mut model = WriteModel::default();
        let entry = model
            .delete(&filter, many, None)
            .map_err(|e| format!("Delete failed: {}", e))?
            .to_owned();
        let request = write_request("delete", "deletes", database, collection, vec![entry])?;
        Ok((
            Self::new("Delete failed", Kind::DeleteCount, database, collection),
            request,
        ))
    }

    /// `collection.countDocuments(filter)`.
    ///
    /// An aggregation, not the metadata `count` command — same as the
    /// `mongodb` crate's `count_documents`, and the distinction is observable:
    /// `count` reports the collection's stored metadata, which can be stale
    /// after an unclean shutdown.
    pub(crate) fn count(
        database: &str,
        collection: &str,
        filter_json: &str,
    ) -> Result<(Self, Request), String> {
        let filter = raw_filter(filter_json)?;
        let mut command = Command::new();
        command
            .count_documents(database, collection, &filter, 0, 0)
            .map_err(|e| format!("Count failed: {}", e))?;
        Ok((
            Self::new("Count failed", Kind::Count, database, collection),
            Request::Body(command.raw().to_owned()),
        ))
    }

    /// `client.listDatabases()` — a JSON array of names.
    pub(crate) fn list_databases() -> Result<(Self, Request), String> {
        let mut command = Command::new();
        command
            .list_databases(true)
            .map_err(|e| format!("List databases failed: {}", e))?;
        Ok((
            Self::new("List databases failed", Kind::ListDatabases, "admin", ""),
            Request::Body(command.raw().to_owned()),
        ))
    }

    /// `db.listCollections()` — a JSON array of names.
    ///
    /// Answers with a cursor, so it takes the same `getMore` continuation a
    /// `find` does once a database holds more collections than one batch.
    pub(crate) fn list_collections(database: &str) -> Result<(Self, Request), String> {
        let mut command = Command::new();
        command
            .list_collections(database, true, &RawDocumentBuf::new())
            .map_err(|e| format!("List collections failed: {}", e))?;
        Ok((
            Self::new(
                "List collections failed",
                Kind::ListCollections { names: Vec::new() },
                database,
                // The cursor `getMore` for listCollections names the pseudo
                // collection `$cmd.listCollections`, which is what the server
                // returns in `ns` and what a follow-up must use.
                "$cmd.listCollections",
            ),
            Request::Body(command.raw().to_owned()),
        ))
    }

    fn new(label: &'static str, kind: Kind, database: &str, collection: &str) -> Self {
        Self {
            label,
            kind,
            database: database.to_owned(),
            collection: collection.to_owned(),
        }
    }

    /// Turn one reply into either a settlement or the next round trip.
    ///
    /// `reply` borrows the core's receive buffer. Everything this returns is
    /// owned, so the caller can release that buffer immediately — which it must
    /// do before the core will accept another command.
    pub(crate) fn interpret(&mut self, reply: &RawDocument) -> Result<Step, String> {
        match &mut self.kind {
            Kind::FindOne => {
                let batch = CursorBatch::parse(reply).map_err(message)?;
                // Bound with a `let` rather than matched inline: `rows()`
                // returns an `impl Iterator` that captures the batch's borrow
                // under Rust 2024's capture rules, and as a tail-position
                // temporary it would outlive `batch` itself.
                let first = batch.rows().next();
                match first {
                    Some(row) => {
                        let document =
                            Document::try_from(row.map_err(message)?).map_err(|e| e.to_string())?;
                        Ok(Step::Settle(Settlement::Json(
                            // The pre-P7 path fell back to `{}` on an encoding
                            // failure rather than rejecting; keep that, so a
                            // document this encoder chokes on fails the same way.
                            serde_json::to_string(&document).unwrap_or_else(|_| "{}".to_string()),
                        )))
                    }
                    None => Ok(Step::Settle(Settlement::Null)),
                }
            }
            Kind::Find { rows } => {
                let batch = CursorBatch::parse(reply).map_err(message)?;
                for row in batch.rows() {
                    rows.push(
                        Document::try_from(row.map_err(message)?).map_err(|e| e.to_string())?,
                    );
                }
                if batch.id != 0 {
                    // A live cursor: fetch the rest. From here on a failure is
                    // a cursor failure, which is the prefix the pre-P7 path's
                    // `try_collect()` error carried.
                    self.label = "Cursor error";
                    let mut command = Command::new();
                    command
                        .get_more(&self.database, &self.collection, batch.id, None, None)
                        .map_err(message)?;
                    return Ok(Step::More(Request::Body(command.raw().to_owned())));
                }
                Ok(Step::Settle(Settlement::Json(
                    serde_json::to_string(rows).unwrap_or_else(|_| "[]".to_string()),
                )))
            }
            Kind::InsertOne { id } => {
                // A duplicate key answers `ok: 1` with a `writeErrors` array,
                // and the pre-P7 path rejected on that, so ask the response
                // parser that knows about write errors. This kind settles a
                // plain id and so never builds a `WriteResult` at all.
                //
                // (Before turnloop-mongodb 0.1.0-alpha.7 this was the ONLY
                // check that caught a write error: `WriteResult::parse` then
                // failed only on `ok: 0`. alpha.7 made `parse` run exactly this
                // `Error::from_response` first — the lenient parse is now
                // `WriteResult::decode` — so the explicit calls at the kinds
                // below are redundant rather than load-bearing. They are kept:
                // they cost one pass over a reply that is already in memory,
                // they produce the identical error, and they keep every kind
                // here rejecting write errors the same visible way.)
                Error::from_response(reply).map_err(message)?;
                Ok(Step::Settle(Settlement::Json(std::mem::take(id))))
            }
            Kind::InsertMany => {
                Error::from_response(reply).map_err(message)?;
                let result = WriteResult::parse(reply).map_err(message)?;
                Ok(Step::Settle(Settlement::Number(result.count as f64)))
            }
            Kind::UpdateCount => {
                Error::from_response(reply).map_err(message)?;
                let result = WriteResult::parse(reply).map_err(message)?;
                Ok(Step::Settle(Settlement::Number(
                    result.modified_count as f64,
                )))
            }
            Kind::DeleteCount => {
                Error::from_response(reply).map_err(message)?;
                let result = WriteResult::parse(reply).map_err(message)?;
                Ok(Step::Settle(Settlement::Number(result.count as f64)))
            }
            Kind::Count => {
                let batch = CursorBatch::parse(reply).map_err(message)?;
                // The `$group`/`$sum` stage produces one row; an empty batch
                // means the filter matched nothing, which counts as zero rather
                // than as a malformed reply.
                let first = batch.rows().next();
                let count = match first {
                    Some(row) => number(row.map_err(message)?, "n").unwrap_or(0.0),
                    None => 0.0,
                };
                Ok(Step::Settle(Settlement::Number(count)))
            }
            Kind::ListDatabases => {
                Error::from_response(reply).map_err(message)?;
                let databases = reply
                    .get_array("databases")
                    .map_err(|_| "Malformed listDatabases reply".to_string())?;
                let mut names = Vec::new();
                for entry in databases {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if let Some(name) = entry.as_document().and_then(|d| d.get_str("name").ok()) {
                        names.push(name.to_owned());
                    }
                }
                Ok(Step::Settle(Settlement::Json(
                    serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string()),
                )))
            }
            Kind::ListCollections { names } => {
                let batch = CursorBatch::parse(reply).map_err(message)?;
                for row in batch.rows() {
                    if let Ok(name) = row.map_err(message)?.get_str("name") {
                        names.push(name.to_owned());
                    }
                }
                if batch.id != 0 {
                    let mut command = Command::new();
                    command
                        .get_more(&self.database, &self.collection, batch.id, None, None)
                        .map_err(message)?;
                    return Ok(Step::More(Request::Body(command.raw().to_owned())));
                }
                Ok(Step::Settle(Settlement::Json(
                    serde_json::to_string(names).unwrap_or_else(|_| "[]".to_string()),
                )))
            }
        }
    }

    /// Settle this operation's promise.
    ///
    /// Strings go through `resolve_with`, never `resolve_string`: this runs in
    /// the loop's completion dispatch, and `resolve_string` would allocate the
    /// runtime string right there. `resolve_with` carries the owned Rust
    /// `String` to the main thread's resolution pump and builds the JS value
    /// there — the #1824 rule, which the `spawn_blocking` path obeyed for the
    /// same reason.
    pub(crate) fn settle(self, promise: JsPromise, settlement: Settlement) {
        match settlement {
            Settlement::Json(text) => {
                promise.resolve_with(move || JsValue::from_string_ptr(alloc_string(&text).as_raw()))
            }
            Settlement::Null => promise.resolve_null(),
            Settlement::Number(n) => promise.resolve_number(n),
        }
    }

    /// Reject with this operation's prefix. `reject_string` already defers the
    /// Error construction to the main thread.
    pub(crate) fn reject(&self, promise: JsPromise, message: &str) {
        promise.reject_string(&format!("{}: {}", self.label, message));
    }
}

/// A filter argument, decoded the way the pre-P7 path decoded it.
///
/// Note the `unwrap_or_else`: an unparseable filter became `{}` there rather
/// than an error, so `find("not json")` returned the whole collection. That is
/// surprising, but it is the established behaviour and changing it here would
/// turn a silently-wide query into a new rejection in existing programs.
fn raw_filter(filter_json: &str) -> Result<RawDocumentBuf, String> {
    let filter: Document = serde_json::from_str(filter_json).unwrap_or_default();
    RawDocumentBuf::try_from(&filter).map_err(|e| e.to_string())
}

/// Give a document the `_id` it will be stored under, and report that id.
///
/// The id is **prepended**, matching the `mongodb` crate's
/// `get_or_prepend_id_field`: field order survives a round trip through the
/// server, so appending would make documents written on this transport differ
/// from documents written on the legacy one.
pub(super) fn insert_document(document: Document) -> Result<(Document, Bson), String> {
    if let Some(existing) = document.get("_id") {
        let id = existing.clone();
        return Ok((document, id));
    }
    let oid = super::next_object_id()
        .ok_or_else(|| "Insert failed: no OS entropy for an ObjectId".to_string())?;
    let id = Bson::ObjectId(oid);
    let mut with_id = Document::new();
    with_id.insert("_id", id.clone());
    for (key, value) in document {
        with_id.insert(key, value);
    }
    Ok((with_id, id))
}

/// Build a write command and its OP_MSG document sequence.
fn write_request(
    which: &'static str,
    sequence: &'static str,
    database: &str,
    collection: &str,
    docs: Vec<RawDocumentBuf>,
) -> Result<Request, String> {
    let mut command = Command::new();
    // `ordered: true` is the npm default and the `mongodb` crate's default.
    let built = match which {
        "insert" => command.insert(database, collection, true, None),
        "update" => command.update(database, collection, true, None),
        _ => command.delete(database, collection, true, None),
    };
    built.map_err(|e| e.to_string())?;
    Ok(Request::WithSequence {
        body: command.raw().to_owned(),
        name: sequence,
        docs,
    })
}

/// A numeric reply field, whatever BSON number type the server chose. The
/// aggregation counter comes back as an Int32 today and as an Int64 once it
/// passes 2^31, and a `get_i32` that silently failed would report zero.
fn number(document: &RawDocument, key: &str) -> Option<f64> {
    match document.get(key).ok().flatten()? {
        bson::raw::RawBsonRef::Int32(v) => Some(f64::from(v)),
        bson::raw::RawBsonRef::Int64(v) => Some(v as f64),
        bson::raw::RawBsonRef::Double(v) => Some(v),
        _ => None,
    }
}

/// `turnloop_mongodb::Error`'s `Display` is `MongoServerError: <errmsg>`, which
/// is the server's own text. Rendering it through one helper keeps every
/// rejection in this module reading the same way.
fn message(error: Error) -> String {
    error.to_string()
}
