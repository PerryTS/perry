//! Type conversions between PostgreSQL types and JSValue

use perry_runtime::{
    js_object_alloc, js_object_get_field, js_object_set_field, js_string_from_bytes, JSValue,
    ObjectHeader, StringHeader,
};
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo};

/// PostgreSQL connection configuration
#[derive(Debug, Clone)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
}

impl Default for PgConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            user: "postgres".to_string(),
            password: String::new(),
            database: None,
        }
    }
}

impl PgConfig {
    /// Build a connection URL from the config
    pub fn to_url(&self) -> String {
        let db_part = self
            .database
            .as_ref()
            .map(|d| format!("/{}", d))
            .unwrap_or_default();
        format!(
            "postgres://{}:{}@{}:{}{}",
            self.user, self.password, self.host, self.port, db_part
        )
    }
}

/// Extract a Rust String from a JSValue that contains a string pointer
unsafe fn jsvalue_to_string(value: JSValue) -> Option<String> {
    if value.is_pointer() {
        let ptr = value.as_pointer() as *const StringHeader;
        if !ptr.is_null() {
            let len = (*ptr).byte_len as usize;
            let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data_ptr, len);
            return Some(String::from_utf8_lossy(bytes).to_string());
        }
    }
    None
}

/// Convert a JSValue config object to PgConfig
///
/// Expected object layout (based on property order in object literal):
/// - field 0: host (string)
/// - field 1: port (number)
/// - field 2: user (string)
/// - field 3: password (string)
/// - field 4: database (string, optional)
///
/// # Safety
/// The config must be a valid JSValue representing an object
pub unsafe fn parse_pg_config(config: JSValue) -> PgConfig {
    let mut result = PgConfig::default();

    // Check if config is a valid object pointer
    if !config.is_pointer() {
        return result;
    }

    let obj_ptr = config.as_pointer() as *const ObjectHeader;
    if obj_ptr.is_null() {
        return result;
    }

    // Extract host (field 0)
    let host_val = js_object_get_field(obj_ptr, 0);
    if let Some(host) = jsvalue_to_string(host_val) {
        result.host = host;
    }

    // Extract port (field 1)
    let port_val = js_object_get_field(obj_ptr, 1);
    if port_val.is_number() {
        result.port = port_val.to_number() as u16;
    }

    // Extract user (field 2)
    let user_val = js_object_get_field(obj_ptr, 2);
    if let Some(user) = jsvalue_to_string(user_val) {
        result.user = user;
    }

    // Extract password (field 3)
    let password_val = js_object_get_field(obj_ptr, 3);
    if let Some(password) = jsvalue_to_string(password_val) {
        result.password = password;
    }

    // Extract database (field 4, optional)
    let database_val = js_object_get_field(obj_ptr, 4);
    if !database_val.is_undefined() && !database_val.is_null() {
        if let Some(database) = jsvalue_to_string(database_val) {
            result.database = Some(database);
        }
    }

    result
}

/// Convert a PostgreSQL row to a JS object
///
/// Returns a pointer to the allocated object
pub fn row_to_js_object(row: &PgRow) -> *mut ObjectHeader {
    let columns = row.columns();
    // Class ID 0 for anonymous object, field count = number of columns
    let obj = js_object_alloc(0, columns.len() as u32);

    for (i, _col) in columns.iter().enumerate() {
        let value = column_value_to_jsvalue(row, i);
        js_object_set_field(obj, i as u32, value);
    }

    obj
}

/// Convert a column value to JSValue
fn column_value_to_jsvalue(row: &PgRow, index: usize) -> JSValue {
    let columns = row.columns();
    let col = &columns[index];
    let type_name = col.type_info().name();

    // Try to get the value based on the column type
    match type_name {
        "INT4" | "INT2" => {
            if let Ok(val) = row.try_get::<i32, _>(index) {
                JSValue::int32(val)
            } else {
                JSValue::null()
            }
        }
        "INT8" => {
            if let Ok(val) = row.try_get::<i64, _>(index) {
                JSValue::number(val as f64)
            } else {
                JSValue::null()
            }
        }
        "FLOAT4" | "FLOAT8" | "NUMERIC" => {
            if let Ok(val) = row.try_get::<f64, _>(index) {
                JSValue::number(val)
            } else {
                JSValue::null()
            }
        }
        "VARCHAR" | "CHAR" | "TEXT" | "BPCHAR" | "NAME" => {
            if let Ok(val) = row.try_get::<String, _>(index) {
                unsafe {
                    let str_ptr = js_string_from_bytes(val.as_ptr(), val.len() as u32);
                    JSValue::string_ptr(str_ptr)
                }
            } else {
                JSValue::null()
            }
        }
        "BOOL" => {
            if let Ok(val) = row.try_get::<bool, _>(index) {
                JSValue::bool(val)
            } else {
                JSValue::null()
            }
        }
        _ => {
            // Try as string fallback
            if let Ok(val) = row.try_get::<String, _>(index) {
                unsafe {
                    let str_ptr = js_string_from_bytes(val.as_ptr(), val.len() as u32);
                    JSValue::string_ptr(str_ptr)
                }
            } else {
                JSValue::null()
            }
        }
    }
}

/// Create a FieldDef object for a column
pub fn column_to_field_def(col: &sqlx::postgres::PgColumn) -> *mut ObjectHeader {
    // FieldDef has these fields:
    // 0: name (string)
    // 1: dataTypeID (number - PostgreSQL OID)
    // 2: tableID (number)
    let obj = js_object_alloc(0, 3);

    // Set name
    let name = col.name();
    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field(obj, 0, JSValue::string_ptr(name_ptr));

    // Set dataTypeID (as string type name for now)
    let type_name = col.type_info().name();
    let type_ptr = js_string_from_bytes(type_name.as_ptr(), type_name.len() as u32);
    js_object_set_field(obj, 1, JSValue::string_ptr(type_ptr));

    // Set tableID (0 for now)
    js_object_set_field(obj, 2, JSValue::number(0.0));

    obj
}
