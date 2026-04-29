//! Query result handling for pg

use perry_runtime::{js_array_alloc, js_array_set, js_object_alloc, js_object_set_field, JSValue};
use sqlx::postgres::{PgColumn, PgRow};

use super::types::{column_to_field_def, row_to_js_object};

/// Convert query results to the pg format: { rows, fields, rowCount, command }
///
/// Returns a JSValue representing a Result object where:
/// - rows: Array of row objects
/// - fields: Array of field metadata objects
/// - rowCount: Number of rows affected/returned
/// - command: SQL command type (SELECT, INSERT, etc.)
pub fn rows_to_pg_result(rows: Vec<PgRow>, columns: &[PgColumn], command: &str) -> JSValue {
    // Create the Result object with 4 fields
    let result_obj = js_object_alloc(0, 4);

    // Create rows array (field 0)
    let rows_array = js_array_alloc(rows.len() as u32);
    for (i, row) in rows.iter().enumerate() {
        let row_obj = row_to_js_object(row);
        js_array_set(
            rows_array,
            i as u32,
            JSValue::object_ptr(row_obj as *mut u8),
        );
    }
    js_object_set_field(result_obj, 0, JSValue::array_ptr(rows_array));

    // Create fields array (field 1)
    let fields_array = js_array_alloc(columns.len() as u32);
    for (i, col) in columns.iter().enumerate() {
        let field_obj = column_to_field_def(col);
        js_array_set(
            fields_array,
            i as u32,
            JSValue::object_ptr(field_obj as *mut u8),
        );
    }
    js_object_set_field(result_obj, 1, JSValue::array_ptr(fields_array));

    // Set rowCount (field 2)
    js_object_set_field(result_obj, 2, JSValue::number(rows.len() as f64));

    // Set command (field 3)
    let cmd_ptr = perry_runtime::js_string_from_bytes(command.as_ptr(), command.len() as u32);
    js_object_set_field(result_obj, 3, JSValue::string_ptr(cmd_ptr));

    JSValue::object_ptr(result_obj as *mut u8)
}

/// Create an empty result for queries that don't return rows
pub fn empty_pg_result(command: &str, row_count: u64) -> JSValue {
    let result_obj = js_object_alloc(0, 4);

    let empty_rows = js_array_alloc(0);
    js_object_set_field(result_obj, 0, JSValue::array_ptr(empty_rows));

    let empty_fields = js_array_alloc(0);
    js_object_set_field(result_obj, 1, JSValue::array_ptr(empty_fields));

    js_object_set_field(result_obj, 2, JSValue::number(row_count as f64));

    let cmd_ptr = perry_runtime::js_string_from_bytes(command.as_ptr(), command.len() as u32);
    js_object_set_field(result_obj, 3, JSValue::string_ptr(cmd_ptr));

    JSValue::object_ptr(result_obj as *mut u8)
}
