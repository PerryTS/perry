//! Query result handling for mysql2

use perry_runtime::{
    js_array_alloc, js_array_push, js_object_alloc, js_object_set_field, js_object_set_keys,
    js_string_from_bytes, JSValue,
};
use sqlx::mysql::{MySqlColumn, MySqlRow};
use sqlx::{Column, Row, TypeInfo};

use super::types::{column_to_field_packet, row_to_js_object};

/// Returns true if the SQL query returns rows (SELECT, SHOW, DESCRIBE, EXPLAIN, etc.)
pub fn is_row_returning_query(sql: &str) -> bool {
    let trimmed = sql.trim_start();
    let upper = trimmed.get(..10).unwrap_or(trimmed).to_uppercase();
    upper.starts_with("SELECT")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESC")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("WITH")
}

/// Raw value types for thread-safe data transfer
#[derive(Clone, Debug)]
pub enum RawValue {
    Null,
    Bool(bool),
    Int32(i32),
    Int64(i64),
    Float64(f64),
    String(String),
    /// DATETIME/TIMESTAMP stored as milliseconds since epoch (for JS Date compatibility)
    DateTimestamp(f64),
}

/// Raw column info for thread-safe data transfer
#[derive(Clone, Debug)]
pub struct RawColumnInfo {
    pub name: String,
    pub type_name: String,
}

/// Raw row data for thread-safe data transfer
#[derive(Clone, Debug)]
pub struct RawRowData {
    pub values: Vec<(String, RawValue)>,
}

/// Raw query result for thread-safe data transfer between threads
#[derive(Clone, Debug)]
pub struct RawQueryResult {
    pub rows: Vec<RawRowData>,
    pub columns: Vec<RawColumnInfo>,
}

/// Query outcome: either rows (SELECT) or execution metadata (INSERT/UPDATE/DELETE)
#[derive(Clone, Debug)]
pub enum QueryOutcome {
    Rows(RawQueryResult),
    Executed {
        affected_rows: u64,
        last_insert_id: u64,
    },
}

impl RawQueryResult {
    /// Extract raw data from sqlx rows (call this on worker thread)
    pub fn from_mysql_rows(rows: Vec<MySqlRow>) -> Self {
        let columns: Vec<RawColumnInfo> = if !rows.is_empty() {
            rows[0]
                .columns()
                .iter()
                .map(|col| RawColumnInfo {
                    name: col.name().to_string(),
                    type_name: col.type_info().name().to_string(),
                })
                .collect()
        } else {
            Vec::new()
        };

        let raw_rows: Vec<RawRowData> = rows
            .iter()
            .map(|row| {
                let values = row
                    .columns()
                    .iter()
                    .enumerate()
                    .map(|(i, col)| {
                        let name = col.name().to_string();
                        let type_name = col.type_info().name();
                        let value = extract_raw_value(row, i, type_name);
                        (name, value)
                    })
                    .collect();
                RawRowData { values }
            })
            .collect();

        RawQueryResult {
            rows: raw_rows,
            columns,
        }
    }

    /// Convert to JSValue (call this on main thread only!)
    pub fn to_jsvalue(&self) -> JSValue {
        // Create the result tuple [rows, fields]
        let mut result_array = js_array_alloc(2);

        // Create rows array
        let mut rows_array = js_array_alloc(self.rows.len() as u32);
        for row in &self.rows {
            let row_obj = raw_row_to_js_object(row, &self.columns);
            rows_array = js_array_push(rows_array, JSValue::object_ptr(row_obj as *mut u8));
        }
        let rows_jsval = JSValue::array_ptr(rows_array);
        result_array = js_array_push(result_array, rows_jsval);

        // Create fields array
        let mut fields_array = js_array_alloc(self.columns.len() as u32);
        for col in &self.columns {
            let field_obj = raw_column_to_field_packet(col);
            fields_array = js_array_push(fields_array, JSValue::object_ptr(field_obj as *mut u8));
        }
        let fields_jsval = JSValue::array_ptr(fields_array);
        result_array = js_array_push(result_array, fields_jsval);

        JSValue::array_ptr(result_array)
    }
}

/// Extract a raw value from a MySQL row (safe to call on any thread)
fn extract_raw_value(row: &MySqlRow, index: usize, type_name: &str) -> RawValue {
    match type_name {
        "INT" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT UNSIGNED" | "TINYINT UNSIGNED"
        | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED" => {
            if let Ok(val) = row.try_get::<i32, _>(index) {
                RawValue::Int32(val)
            } else {
                RawValue::Null
            }
        }
        "BIGINT" | "BIGINT UNSIGNED" => {
            if let Ok(val) = row.try_get::<i64, _>(index) {
                RawValue::Int64(val)
            } else {
                RawValue::Null
            }
        }
        "FLOAT" | "DOUBLE" | "DECIMAL" => {
            if let Ok(val) = row.try_get::<f64, _>(index) {
                RawValue::Float64(val)
            } else {
                RawValue::Null
            }
        }
        "BOOLEAN" | "BOOL" => {
            if let Ok(val) = row.try_get::<bool, _>(index) {
                RawValue::Bool(val)
            } else {
                RawValue::Null
            }
        }
        "DATETIME" | "TIMESTAMP" => {
            if let Ok(val) = row.try_get::<chrono::NaiveDateTime, _>(index) {
                RawValue::String(val.format("%Y-%m-%d %H:%M:%S").to_string())
            } else {
                RawValue::Null
            }
        }
        "DATE" => {
            if let Ok(val) = row.try_get::<chrono::NaiveDate, _>(index) {
                RawValue::String(val.format("%Y-%m-%d").to_string())
            } else {
                RawValue::Null
            }
        }
        "TIME" => {
            if let Ok(val) = row.try_get::<chrono::NaiveTime, _>(index) {
                RawValue::String(val.format("%H:%M:%S").to_string())
            } else {
                RawValue::Null
            }
        }
        _ => {
            // Try as string first for VARCHAR, CHAR, TEXT, ENUM, SET, etc.
            if let Ok(val) = row.try_get::<String, _>(index) {
                RawValue::String(val)
            } else if let Ok(val) = row.try_get::<Vec<u8>, _>(index) {
                // Fallback for BLOB/BINARY types — try UTF-8 conversion
                RawValue::String(String::from_utf8_lossy(&val).to_string())
            } else {
                RawValue::Null
            }
        }
    }
}

/// Convert a raw row to a JS object (must be called on main thread)
fn raw_row_to_js_object(
    row: &RawRowData,
    _columns: &[RawColumnInfo],
) -> *mut perry_runtime::ObjectHeader {
    let obj = js_object_alloc(0, row.values.len() as u32);
    let mut keys_array = js_array_alloc(row.values.len() as u32);

    for (i, (name, value)) in row.values.iter().enumerate() {
        // Set the field value
        let jsval = raw_value_to_jsvalue(value);
        js_object_set_field(obj, i as u32, jsval);

        // Add column name to keys array
        let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let name_jsval = JSValue::string_ptr(name_ptr);
        keys_array = js_array_push(keys_array, name_jsval);
    }

    js_object_set_keys(obj, keys_array);
    obj
}

/// Convert a raw value to JSValue (must be called on main thread)
fn raw_value_to_jsvalue(value: &RawValue) -> JSValue {
    match value {
        RawValue::Null => JSValue::null(),
        RawValue::Bool(b) => JSValue::bool(*b),
        // WORKAROUND for Bug #46: Use JSValue::number instead of JSValue::int32
        // to avoid INT32_TAG NaN-boxing issues with arithmetic and comparisons
        RawValue::Int32(i) => JSValue::number(*i as f64),
        RawValue::Int64(i) => JSValue::number(*i as f64),
        RawValue::Float64(f) => JSValue::number(*f),
        RawValue::String(s) => {
            let str_ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            JSValue::string_ptr(str_ptr)
        }
        RawValue::DateTimestamp(millis) => {
            // Store as f64 timestamp — matches JS Date behavior.
            // new Date(row.datetime) will receive the timestamp directly.
            JSValue::number(*millis)
        }
    }
}

/// Convert a raw column to a field packet (must be called on main thread)
fn raw_column_to_field_packet(col: &RawColumnInfo) -> *mut perry_runtime::ObjectHeader {
    let obj = js_object_alloc(0, 3);
    let mut keys_array = js_array_alloc(3);

    // Set name
    let name_ptr = js_string_from_bytes(col.name.as_ptr(), col.name.len() as u32);
    js_object_set_field(obj, 0, JSValue::string_ptr(name_ptr));
    let key0 = js_string_from_bytes("name".as_ptr(), 4);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key0));

    // Set type
    let type_ptr = js_string_from_bytes(col.type_name.as_ptr(), col.type_name.len() as u32);
    js_object_set_field(obj, 1, JSValue::string_ptr(type_ptr));
    let key1 = js_string_from_bytes("type".as_ptr(), 4);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key1));

    // Set length (0 for now)
    js_object_set_field(obj, 2, JSValue::number(0.0));
    let key2 = js_string_from_bytes("length".as_ptr(), 6);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key2));

    js_object_set_keys(obj, keys_array);
    obj
}

/// Convert query results to the mysql2 format: [rows, fields]
///
/// Returns a JSValue representing a 2-element array where:
/// - index 0: Array of row objects (RowDataPacket[])
/// - index 1: Array of field metadata objects (FieldPacket[])
pub fn rows_to_result_tuple(rows: Vec<MySqlRow>, columns: &[MySqlColumn]) -> JSValue {
    // Create the result tuple [rows, fields]
    let mut result_array = js_array_alloc(2);

    // Create rows array
    let mut rows_array = js_array_alloc(rows.len() as u32);
    for row in rows.iter() {
        let row_obj = row_to_js_object(row);
        rows_array = js_array_push(rows_array, JSValue::object_ptr(row_obj as *mut u8));
    }
    let rows_jsval = JSValue::array_ptr(rows_array);
    result_array = js_array_push(result_array, rows_jsval);

    // Create fields array
    let mut fields_array = js_array_alloc(columns.len() as u32);
    for col in columns.iter() {
        let field_obj = column_to_field_packet(col);
        fields_array = js_array_push(fields_array, JSValue::object_ptr(field_obj as *mut u8));
    }
    let fields_jsval = JSValue::array_ptr(fields_array);
    result_array = js_array_push(result_array, fields_jsval);

    JSValue::array_ptr(result_array)
}

impl QueryOutcome {
    pub fn to_jsvalue(&self) -> JSValue {
        match self {
            QueryOutcome::Rows(raw) => raw.to_jsvalue(),
            QueryOutcome::Executed {
                affected_rows,
                last_insert_id,
            } => affected_rows_result(*affected_rows, *last_insert_id),
        }
    }
}

/// Create an empty result (for queries that don't return rows, like INSERT/UPDATE)
pub fn empty_result() -> JSValue {
    let mut result_array = js_array_alloc(2);
    let empty_rows = js_array_alloc(0);
    let empty_fields = js_array_alloc(0);
    result_array = js_array_push(result_array, JSValue::array_ptr(empty_rows));
    result_array = js_array_push(result_array, JSValue::array_ptr(empty_fields));
    JSValue::array_ptr(result_array)
}

/// Create a result with affected rows info (for INSERT/UPDATE/DELETE)
///
/// mysql2 returns a ResultSetHeader for non-SELECT queries with:
/// - affectedRows
/// - insertId
/// - warningStatus
pub fn affected_rows_result(affected: u64, last_insert_id: u64) -> JSValue {
    // Create result tuple [header, fields]
    let mut result_array = js_array_alloc(2);

    // Create ResultSetHeader object
    let header = js_object_alloc(0, 3);

    // Create keys array for property name lookup
    let mut keys_array = js_array_alloc(3);

    // Set affectedRows (field index 0)
    js_object_set_field(header, 0, JSValue::number(affected as f64));
    let key0 = js_string_from_bytes("affectedRows".as_ptr(), 12);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key0));

    // Set insertId (field index 1)
    js_object_set_field(header, 1, JSValue::number(last_insert_id as f64));
    let key1 = js_string_from_bytes("insertId".as_ptr(), 8);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key1));

    // Set warningStatus (field index 2)
    js_object_set_field(header, 2, JSValue::number(0.0));
    let key2 = js_string_from_bytes("warningStatus".as_ptr(), 13);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key2));

    // Attach keys to header object
    js_object_set_keys(header, keys_array);

    result_array = js_array_push(result_array, JSValue::object_ptr(header as *mut u8));

    // Empty fields array
    let empty_fields = js_array_alloc(0);
    result_array = js_array_push(result_array, JSValue::array_ptr(empty_fields));

    JSValue::array_ptr(result_array)
}
