//! Owned result materialization for the turnloop transport.
//!
//! Two rules shape this file, and both come from the transport change.
//!
//! * **Every event `turnloop_postgres` hands back borrows the core's receive
//!   buffer** and is invalidated by the next mutable call on the connection. A
//!   row therefore has to be copied out *inside* the sink call, into the owned
//!   [`Cell`]/[`ColumnMeta`] values in this file, before the driver advances.
//! * **No JS value may be built in the sink.** [`QueryResult::into_js`] is the
//!   only function here that touches the runtime, and it runs on the main
//!   thread from inside a `JsPromise::resolve_with` closure.
//!
//! The object it builds is deliberately identical to the one the `sqlx` path
//! builds in `lib.rs` — same keys, same order, same values — because a program
//! that ran before P7 must not be able to tell which transport answered it. The
//! key lists are the *same constants* (`crate::RESULT_KEYS`, `crate::FIELD_KEYS`)
//! rather than a second copy, so the two builders cannot drift apart silently.

use perry_ffi::{
    alloc_string, build_object_shape, js_array_alloc, js_array_push, js_object_alloc_with_shape,
    js_object_set_field, JsValue, ObjectHeader,
};
use turnloop_postgres::types::{decode, Value};
use turnloop_postgres::{Error, Fields, Row};

/// PostgreSQL type OIDs this binding gives a JS value other than `null`.
///
/// Named rather than inlined because the policy below is the JS-visible
/// contract: `column_value_to_jsvalue` in `lib.rs` selects the same set by
/// sqlx's *type name*, and these are the OIDs those names denote.
mod oid {
    pub const BOOL: u32 = 16;
    /// `"char"`, the single-byte internal type — sqlx calls this `CHAR`.
    pub const CHAR: u32 = 18;
    pub const NAME: u32 = 19;
    pub const INT8: u32 = 20;
    pub const INT2: u32 = 21;
    pub const INT4: u32 = 23;
    pub const TEXT: u32 = 25;
    pub const FLOAT4: u32 = 700;
    pub const FLOAT8: u32 = 701;
    /// `character(n)` — sqlx calls this `BPCHAR`.
    pub const BPCHAR: u32 = 1042;
    pub const VARCHAR: u32 = 1043;
    pub const NUMERIC: u32 = 1700;
}

/// One column's value, decided on the agent thread from owned bytes.
///
/// This exists so the decode happens where the wire bytes are still valid and
/// the *JS allocation* happens later, on the main thread. It is also what makes
/// the conversion policy testable without a PostgreSQL server: the mapping from
/// (OID, wire bytes) to `Cell` is a pure function.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cell {
    Null,
    Bool(bool),
    Int(i32),
    Number(f64),
    Text(String),
}

impl Cell {
    /// The binding's wire → JS policy, by OID.
    ///
    /// Matched on the OID rather than on the decoded [`Value`] variant on
    /// purpose: `decode` renders an *unknown* OID's text as `Value::Text`, and
    /// under sqlx those columns came back `null` (`try_get::<String, _>` on,
    /// say, a `timestamptz` is a decode error, which the old binding swallowed
    /// into `JsValue::NULL`). Keying on the variant would silently start
    /// returning strings for every type Perry does not claim to support.
    ///
    /// The one deliberate difference from the sqlx path is `NUMERIC`: sqlx has
    /// no `f64` decoder for it without the `bigdecimal` feature, so every
    /// numeric column was `null`. `turnloop_postgres` hands back the exact
    /// decimal text, so this returns a real number — the same policy the
    /// binding already applied to `FLOAT4`/`FLOAT8`. `NaN`, `Infinity` and
    /// `-Infinity` parse to the matching JS numbers.
    pub(crate) fn from_wire(oid: u32, format: i16, bytes: Option<&[u8]>) -> Self {
        let Ok(value) = decode(oid, format, bytes) else {
            // A cell this codec cannot parse must not fail the whole statement:
            // under sqlx a per-column decode error also became `null`, and a
            // single bad value taking out the surrounding result set would be a
            // new failure mode for programs that already ran.
            return Self::Null;
        };
        match oid {
            oid::BOOL => match value {
                Value::Bool(b) => Self::Bool(b),
                _ => Self::Null,
            },
            oid::INT2 | oid::INT4 => match value {
                Value::Int(n) => Self::Int(n),
                _ => Self::Null,
            },
            // Lossy above 2^53, exactly as before: the sqlx path read an `i64`
            // and cast it to `f64`. node-pg returns int8 as a decimal *string*;
            // Perry's divergence there is older than this transport and is not
            // moved here.
            oid::INT8 => match value {
                Value::Int8(n) => Self::Number(n as f64),
                _ => Self::Null,
            },
            oid::FLOAT4 | oid::FLOAT8 => match value {
                Value::Float(f) => Self::Number(f),
                _ => Self::Null,
            },
            oid::NUMERIC => match value {
                Value::Numeric(text) => match text.parse::<f64>() {
                    Ok(n) => Self::Number(n),
                    Err(_) => Self::Null,
                },
                _ => Self::Null,
            },
            oid::CHAR | oid::NAME | oid::TEXT | oid::BPCHAR | oid::VARCHAR => match value {
                Value::Text(text) => Self::Text(text.into_owned()),
                _ => Self::Null,
            },
            _ => Self::Null,
        }
    }

    /// Main thread only — `alloc_string` allocates in the agent's arena.
    fn into_js(self) -> JsValue {
        match self {
            Self::Null => JsValue::NULL,
            Self::Bool(b) => JsValue::from_bool(b),
            Self::Int(n) => JsValue::from_int32(n),
            Self::Number(n) => JsValue::from_number(n),
            Self::Text(text) => JsValue::from_string_ptr(alloc_string(&text).as_raw()),
        }
    }
}

/// One column of a RowDescription, owned.
///
/// `data_type_size` and `data_type_modifier` are deliberately **not** carried:
/// `result.fields[i]` reports `-1` for both, which is what the sqlx binding
/// reported (sqlx 0.8/0.9 does not expose them). `turnloop_postgres` does hand
/// back the real values — adopting them would be a JS-visible change and so
/// belongs in its own commit, not in a transport migration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ColumnMeta {
    pub(crate) name: String,
    pub(crate) table_id: u32,
    pub(crate) column_id: i16,
    pub(crate) data_type_id: u32,
    /// 0 = text, 1 = binary. This binding asks for text results, so it is 0;
    /// it is carried because [`Cell::from_wire`] must decode in the format the
    /// server actually used, not in the one we asked for.
    pub(crate) format: i16,
}

/// Copy a RowDescription out of the core's buffer.
pub(crate) fn columns_from_fields(fields: Fields<'_>) -> Result<Vec<ColumnMeta>, Error> {
    let mut columns = Vec::with_capacity(fields.len());
    for field in fields {
        // `Fields::parse` already walked and validated every field, so this
        // cannot fail in practice; treating it as a protocol error rather than
        // skipping the column keeps a corrupt description from producing a row
        // object with silently missing keys.
        let field = field?;
        columns.push(ColumnMeta {
            name: field.name.to_string(),
            table_id: field.table_id,
            column_id: field.column_id,
            data_type_id: field.data_type_id,
            format: field.format,
        });
    }
    Ok(columns)
}

/// Copy one DataRow out of the core's buffer, applying the conversion policy.
///
/// A row with more values than the description has columns keeps the extra
/// values as `null` rather than panicking: the OID lookup is what needs the
/// column, and a mismatch is a protocol bug, not a reason to abort the process.
pub(crate) fn cells_from_row(columns: &[ColumnMeta], row: Row<'_>) -> Result<Vec<Cell>, Error> {
    let mut cells = Vec::with_capacity(row.len());
    for (index, value) in row.enumerate() {
        let bytes = value?;
        cells.push(match columns.get(index) {
            Some(column) => Cell::from_wire(column.data_type_id, column.format, bytes),
            None => Cell::Null,
        });
    }
    Ok(cells)
}

/// Everything one statement produced, owned and ready to cross to the main
/// thread inside a `JsPromise::resolve_with` closure.
pub(crate) struct QueryResult {
    pub(crate) columns: Vec<ColumnMeta>,
    pub(crate) rows: Vec<Vec<Cell>>,
    /// The first whitespace-delimited word of the SQL, uppercased — taken from
    /// the *statement text*, not from the server's CommandComplete tag, because
    /// that is where the sqlx path took it and the two differ (`SELECT ... FOR
    /// UPDATE` tags as `SELECT`, but `WITH ... INSERT` tags as `INSERT`).
    pub(crate) command: String,
    /// `result.rowCount`. For the row-returning shapes this is the number of
    /// rows collected; for the `execute` shape it is the server's affected-row
    /// count. See `ResultKind` in the parent module.
    pub(crate) row_count: f64,
}

impl QueryResult {
    /// Build pg's `{ rows, fields, rowCount, command }`. **Main thread only.**
    ///
    /// Mirrors `lib.rs`'s `rows_to_pg_result` field for field, including one
    /// non-obvious behaviour worth stating plainly: the sqlx path derived its
    /// column list from `rows[0]`, so a statement that returns **zero rows
    /// reports `fields: []`** even though the server sent a RowDescription.
    /// That is reproduced here rather than fixed, because `result.fields.length`
    /// is observable and a program may already branch on it.
    pub(crate) fn into_js(self) -> JsValue {
        let (packed, shape_id) = build_object_shape(&crate::RESULT_KEYS);
        // SAFETY: the shape was built from exactly these four keys.
        let result = unsafe {
            js_object_alloc_with_shape(shape_id, 4, packed.as_ptr(), packed.len() as u32)
        };

        let had_rows = !self.rows.is_empty();
        let mut rows_arr = unsafe { js_array_alloc(self.rows.len() as u32) };
        let names: Vec<&str> = self.columns.iter().map(|c| c.name.as_str()).collect();
        for row in self.rows {
            let row_obj = row_to_js_object(&names, row);
            rows_arr = unsafe { js_array_push(rows_arr, JsValue::from_object_ptr(row_obj)) };
        }
        unsafe { js_object_set_field(result, 0, JsValue::from_object_ptr(rows_arr)) };

        let fields: &[ColumnMeta] = if had_rows { &self.columns } else { &[] };
        let mut fields_arr = unsafe { js_array_alloc(fields.len() as u32) };
        for column in fields {
            let field_obj = column_to_field_def(column);
            fields_arr = unsafe { js_array_push(fields_arr, JsValue::from_object_ptr(field_obj)) };
        }
        unsafe { js_object_set_field(result, 1, JsValue::from_object_ptr(fields_arr)) };

        unsafe {
            js_object_set_field(result, 2, JsValue::from_number(self.row_count));
            let command = alloc_string(&self.command);
            js_object_set_field(result, 3, JsValue::from_string_ptr(command.as_raw()));
        }
        JsValue::from_object_ptr(result)
    }
}

/// Twin of `lib.rs`'s `row_to_js_object`, over owned cells. Main thread only.
fn row_to_js_object(names: &[&str], cells: Vec<Cell>) -> *mut ObjectHeader {
    let (packed, shape_id) = build_object_shape(names);
    // SAFETY: the shape was built from exactly `names`.
    let obj = unsafe {
        js_object_alloc_with_shape(
            shape_id,
            names.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    for (index, cell) in cells.into_iter().enumerate() {
        if index >= names.len() {
            // More values than the description described. The object was
            // allocated for `names.len()` fields; writing past that would
            // corrupt the next object's header.
            break;
        }
        // SAFETY: `index` is below the field count the object was allocated with.
        unsafe { js_object_set_field(obj, index as u32, cell.into_js()) };
    }
    obj
}

/// Twin of `lib.rs`'s `column_to_field_def`, over owned metadata.
fn column_to_field_def(column: &ColumnMeta) -> *mut ObjectHeader {
    let (packed, shape_id) = build_object_shape(&crate::FIELD_KEYS);
    // SAFETY: the shape was built from exactly these seven keys.
    let obj =
        unsafe { js_object_alloc_with_shape(shape_id, 7, packed.as_ptr(), packed.len() as u32) };
    let name = alloc_string(&column.name);
    let format = alloc_string("text");
    unsafe {
        js_object_set_field(obj, 0, JsValue::from_string_ptr(name.as_raw()));
        js_object_set_field(obj, 1, JsValue::from_number(f64::from(column.table_id)));
        js_object_set_field(obj, 2, JsValue::from_number(f64::from(column.column_id)));
        js_object_set_field(obj, 3, JsValue::from_number(f64::from(column.data_type_id)));
        // -1/-1 are the sqlx binding's "unknown/variable" sentinels; see the
        // note on `ColumnMeta`.
        js_object_set_field(obj, 4, JsValue::from_number(-1.0));
        js_object_set_field(obj, 5, JsValue::from_number(-1.0));
        js_object_set_field(obj, 6, JsValue::from_string_ptr(format.as_raw()));
    }
    obj
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conversion policy is the JS-visible contract. Each of these cells is
    /// a shape a program can already observe today, so a change here is a
    /// change to what user code receives.
    #[test]
    fn the_scalar_conversion_policy_matches_the_sqlx_binding() {
        assert_eq!(Cell::from_wire(oid::BOOL, 0, Some(b"t")), Cell::Bool(true));
        assert_eq!(Cell::from_wire(oid::BOOL, 0, Some(b"f")), Cell::Bool(false));
        assert_eq!(Cell::from_wire(oid::INT4, 0, Some(b"42")), Cell::Int(42));
        assert_eq!(Cell::from_wire(oid::INT2, 0, Some(b"-7")), Cell::Int(-7));
        assert_eq!(
            Cell::from_wire(oid::FLOAT8, 0, Some(b"1.5")),
            Cell::Number(1.5)
        );
        assert_eq!(
            Cell::from_wire(oid::TEXT, 0, Some(b"hello")),
            Cell::Text("hello".into())
        );
        assert_eq!(
            Cell::from_wire(oid::VARCHAR, 0, Some(b"hello")),
            Cell::Text("hello".into())
        );
    }

    /// A SQL NULL is `null` whatever the column type is. Under sqlx this came
    /// out of `try_get`'s `Option` handling; here it is the wire's `-1` length,
    /// which `decode` turns into `Value::Null` for every OID.
    #[test]
    fn a_sql_null_is_js_null_for_every_supported_type() {
        for oid in [
            oid::BOOL,
            oid::INT2,
            oid::INT4,
            oid::INT8,
            oid::FLOAT8,
            oid::NUMERIC,
            oid::TEXT,
            9999,
        ] {
            assert_eq!(Cell::from_wire(oid, 0, None), Cell::Null, "oid {oid}");
        }
    }

    /// int8 keeps its pre-P7 precision loss. Stating it as a test rather than a
    /// comment because it is the kind of divergence that gets "fixed" by
    /// accident: node-pg returns a decimal *string* here, and switching to that
    /// would silently change every `typeof row.id` in existing code.
    #[test]
    fn int8_is_a_lossy_number_exactly_as_before() {
        assert_eq!(Cell::from_wire(oid::INT8, 0, Some(b"9007199254740993")), {
            // 2^53 + 1 is not representable; the old binding's `n as f64` lost
            // it the same way.
            Cell::Number(9007199254740993i64 as f64)
        });
        assert_eq!(Cell::from_wire(oid::INT8, 0, Some(b"5")), Cell::Number(5.0));
    }

    /// The one deliberate improvement. Worth pinning: if a future refactor
    /// routes numeric back through an unknown-OID path it becomes `null` again
    /// and no other test would notice.
    #[test]
    fn numeric_now_decodes_to_a_number_where_sqlx_produced_null() {
        assert_eq!(
            Cell::from_wire(oid::NUMERIC, 0, Some(b"1.2300")),
            Cell::Number(1.23)
        );
        assert!(matches!(
            Cell::from_wire(oid::NUMERIC, 0, Some(b"NaN")),
            Cell::Number(n) if n.is_nan()
        ));
    }

    /// Everything Perry does not claim to support stays `null`. `decode` is
    /// happy to hand back text for these; the policy, not the codec, is what
    /// keeps the JS value stable.
    #[test]
    fn an_unsupported_type_is_null_rather_than_its_text() {
        // timestamptz, json, uuid, bytea, and an OID no codec knows.
        for oid in [1184u32, 114, 2950, 17, 424242] {
            assert_eq!(
                Cell::from_wire(oid, 0, Some(b"2020-01-01 00:00:00+00")),
                Cell::Null,
                "oid {oid}"
            );
        }
    }

    /// A malformed cell must not take the statement down with it.
    #[test]
    fn an_undecodable_cell_is_null_rather_than_an_error() {
        // "maybe" is not a valid boolean text representation.
        assert_eq!(Cell::from_wire(oid::BOOL, 0, Some(b"maybe")), Cell::Null);
        // Invalid UTF-8 in a text column.
        assert_eq!(
            Cell::from_wire(oid::TEXT, 0, Some(&[0xff, 0xfe])),
            Cell::Null
        );
    }

    /// Binary format still decodes, even though this binding asks for text.
    /// `ColumnMeta::format` carries the server's answer rather than our request
    /// so a server that ignores the request cannot produce garbage cells.
    #[test]
    fn a_binary_format_column_decodes_by_the_same_policy() {
        assert_eq!(
            Cell::from_wire(oid::INT4, 1, Some(&42i32.to_be_bytes())),
            Cell::Int(42)
        );
        assert_eq!(Cell::from_wire(oid::BOOL, 1, Some(&[1])), Cell::Bool(true));
    }
}
