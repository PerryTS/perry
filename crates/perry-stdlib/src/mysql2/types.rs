//! Type conversions between MySQL types and JSValue

use perry_runtime::{
    js_array_alloc, js_array_push, js_object_alloc, js_object_get_field_by_name,
    js_object_set_field, js_object_set_keys, js_string_from_bytes, JSValue, ObjectHeader,
    StringHeader,
};
use sqlx::mysql::MySqlRow;
use sqlx::{Column, Row, TypeInfo};

/// MySQL connection configuration
#[derive(Debug, Clone)]
pub struct MySqlConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
}

impl Default for MySqlConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: String::new(),
            database: None,
        }
    }
}

impl MySqlConfig {
    /// Build a connection URL from the config
    pub fn to_url(&self) -> String {
        let db_part = self
            .database
            .as_ref()
            .map(|d| format!("/{}", d))
            .unwrap_or_default();
        // URL-encode password to handle special characters (e.g., # @ : /)
        let encoded_password: String = self
            .password
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                c => format!("%{:02X}", c as u32),
            })
            .collect();
        let url = format!(
            "mysql://{}:{}@{}:{}{}?ssl-mode=disabled",
            self.user, encoded_password, self.host, self.port, db_part
        );
        url
    }
}

/// Extract a Rust String from a JSValue that contains a string pointer
unsafe fn jsvalue_to_string(value: JSValue) -> Option<String> {
    // Check for NaN-boxed string (STRING_TAG = 0x7FFF)
    if value.is_string() {
        let ptr = value.as_string_ptr();
        if !ptr.is_null() {
            let len = (*ptr).byte_len as usize;
            let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data_ptr, len);
            return Some(String::from_utf8_lossy(bytes).to_string());
        }
    }
    // Also check for raw pointer (POINTER_TAG = 0x7FFD) pointing to a string
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

/// Helper to create a string key for field lookup
unsafe fn make_key(s: &str) -> *const StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Convert a JSValue config object to MySqlConfig
///
/// Supports two formats:
/// 1. URI format: { uri: "mysql://user:pass@host:port/database" }
/// 2. Individual fields: { host, port, user, password, database }
///
/// # Safety
/// The config must be a valid JSValue representing an object
pub unsafe fn parse_mysql_config(config: JSValue) -> MySqlConfig {
    let mut result = MySqlConfig::default();

    // Check if config is a valid object pointer (NaN-boxed or raw pointer)
    let obj_ptr: *const ObjectHeader = if config.is_pointer() {
        // NaN-boxed pointer (POINTER_TAG = 0x7FFD)
        config.as_pointer()
    } else if !config.is_null() && !config.is_undefined() && !config.is_bool() {
        // Perry may pass objects as raw pointers (bits directly hold the address)
        // This happens when object values are passed to C functions without NaN-boxing
        let raw_bits = config.bits();
        // Valid pointer: non-zero and looks like a heap address (reasonable range)
        if raw_bits == 0 || raw_bits > 0x0000_7FFF_FFFF_FFFF {
            return result;
        }
        raw_bits as *const ObjectHeader
    } else {
        return result;
    };
    if obj_ptr.is_null() {
        return result;
    }

    // Try to get the URI field first
    let uri_key = make_key("uri");
    let uri_val = js_object_get_field_by_name(obj_ptr, uri_key);
    if let Some(uri_str) = jsvalue_to_string(uri_val) {
        if let Some(parsed) = parse_mysql_uri(&uri_str) {
            return parsed;
        }
    }

    // Extract host by name
    let host_key = make_key("host");
    let host_val = js_object_get_field_by_name(obj_ptr, host_key);
    if let Some(host) = jsvalue_to_string(host_val) {
        result.host = host;
    }

    // Extract port by name
    let port_key = make_key("port");
    let port_val = js_object_get_field_by_name(obj_ptr, port_key);
    if port_val.is_number() {
        result.port = port_val.to_number() as u16;
    }

    // Extract user by name
    let user_key = make_key("user");
    let user_val = js_object_get_field_by_name(obj_ptr, user_key);
    if let Some(user) = jsvalue_to_string(user_val) {
        result.user = user;
    }

    // Extract password by name
    let password_key = make_key("password");
    let password_val = js_object_get_field_by_name(obj_ptr, password_key);
    if let Some(password) = jsvalue_to_string(password_val) {
        result.password = password;
    }

    // Extract database by name (optional)
    let database_key = make_key("database");
    let database_val = js_object_get_field_by_name(obj_ptr, database_key);
    if !database_val.is_undefined() && !database_val.is_null() {
        if let Some(database) = jsvalue_to_string(database_val) {
            result.database = Some(database);
        }
    }

    result
}

/// Parse a MySQL connection URI into MySqlConfig
/// Format: mysql://user:password@host:port/database
fn parse_mysql_uri(uri: &str) -> Option<MySqlConfig> {
    let uri = uri.strip_prefix("mysql://")?;

    // Split by @ to separate credentials from host
    let (credentials, host_part) = if let Some(idx) = uri.rfind('@') {
        (&uri[..idx], &uri[idx + 1..])
    } else {
        ("", uri)
    };

    // Parse credentials (user:password)
    let (user, password) = if let Some(idx) = credentials.find(':') {
        (
            credentials[..idx].to_string(),
            credentials[idx + 1..].to_string(),
        )
    } else {
        (credentials.to_string(), String::new())
    };

    // Parse host:port/database
    let (host_port, database) = if let Some(idx) = host_part.find('/') {
        (&host_part[..idx], Some(host_part[idx + 1..].to_string()))
    } else {
        (host_part, None)
    };

    // Parse host:port
    let (host, port) = if let Some(idx) = host_port.rfind(':') {
        let port_str = &host_port[idx + 1..];
        let port = port_str.parse().unwrap_or(3306);
        (host_port[..idx].to_string(), port)
    } else {
        (host_port.to_string(), 3306)
    };

    Some(MySqlConfig {
        host,
        port,
        user,
        password,
        database,
    })
}

/// Convert a MySQL row to a JS object (RowDataPacket)
///
/// Returns a pointer to the allocated object
pub fn row_to_js_object(row: &MySqlRow) -> *mut ObjectHeader {
    let columns = row.columns();
    // Class ID 0 for anonymous object, field count = number of columns
    let obj = js_object_alloc(0, columns.len() as u32);

    // Create keys array for property name lookup
    let mut keys_array = js_array_alloc(columns.len() as u32);

    for (i, col) in columns.iter().enumerate() {
        // Set the field value
        let value = column_value_to_jsvalue(row, i);
        js_object_set_field(obj, i as u32, value);

        // Add column name to keys array (NaN-boxed string pointer)
        let col_name = col.name();
        let name_ptr = js_string_from_bytes(col_name.as_ptr(), col_name.len() as u32);
        let name_jsval = JSValue::string_ptr(name_ptr);
        keys_array = js_array_push(keys_array, name_jsval);
    }

    // Attach keys array to object for property name lookup
    js_object_set_keys(obj, keys_array);

    obj
}

/// Convert a column value to JSValue
fn column_value_to_jsvalue(row: &MySqlRow, index: usize) -> JSValue {
    let columns = row.columns();
    let col = &columns[index];
    let type_name = col.type_info().name();

    // Try to get the value based on the column type
    match type_name {
        "INT" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT UNSIGNED" | "TINYINT UNSIGNED"
        | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED" => {
            if let Ok(val) = row.try_get::<i32, _>(index) {
                JSValue::int32(val)
            } else {
                JSValue::null()
            }
        }
        "BIGINT" | "BIGINT UNSIGNED" => {
            if let Ok(val) = row.try_get::<i64, _>(index) {
                JSValue::number(val as f64)
            } else {
                JSValue::null()
            }
        }
        "FLOAT" | "DOUBLE" | "DECIMAL" => {
            if let Ok(val) = row.try_get::<f64, _>(index) {
                JSValue::number(val)
            } else {
                JSValue::null()
            }
        }
        "VARCHAR" | "CHAR" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "TINYTEXT" | "ENUM" | "SET" => {
            if let Ok(val) = row.try_get::<String, _>(index) {
                let str_ptr = js_string_from_bytes(val.as_ptr(), val.len() as u32);
                JSValue::string_ptr(str_ptr)
            } else {
                JSValue::null()
            }
        }
        "BOOLEAN" | "BOOL" => {
            if let Ok(val) = row.try_get::<bool, _>(index) {
                JSValue::bool(val)
            } else {
                JSValue::null()
            }
        }
        "DATETIME" | "TIMESTAMP" => {
            if let Ok(val) = row.try_get::<chrono::NaiveDateTime, _>(index) {
                let s = val.format("%Y-%m-%d %H:%M:%S").to_string();
                let str_ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                JSValue::string_ptr(str_ptr)
            } else {
                JSValue::null()
            }
        }
        "DATE" => {
            if let Ok(val) = row.try_get::<chrono::NaiveDate, _>(index) {
                let s = val.format("%Y-%m-%d").to_string();
                let str_ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                JSValue::string_ptr(str_ptr)
            } else {
                JSValue::null()
            }
        }
        "TIME" => {
            if let Ok(val) = row.try_get::<chrono::NaiveTime, _>(index) {
                let s = val.format("%H:%M:%S").to_string();
                let str_ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                JSValue::string_ptr(str_ptr)
            } else {
                JSValue::null()
            }
        }
        _ => {
            // Try as string first for unknown types
            if let Ok(val) = row.try_get::<String, _>(index) {
                let str_ptr = js_string_from_bytes(val.as_ptr(), val.len() as u32);
                JSValue::string_ptr(str_ptr)
            } else if let Ok(val) = row.try_get::<Vec<u8>, _>(index) {
                // Fallback for BLOB/BINARY types — try UTF-8 conversion
                let s = String::from_utf8_lossy(&val);
                let str_ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                JSValue::string_ptr(str_ptr)
            } else {
                JSValue::null()
            }
        }
    }
}

/// Create a FieldPacket object for a column
pub fn column_to_field_packet(col: &sqlx::mysql::MySqlColumn) -> *mut ObjectHeader {
    // FieldPacket has these fields:
    // 0: name (string)
    // 1: type (number - MySQL type code)
    // 2: length (number)
    let obj = js_object_alloc(0, 3);

    // Create keys array for property name lookup
    let mut keys_array = js_array_alloc(3);

    // Set name
    let name = col.name();
    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field(obj, 0, JSValue::string_ptr(name_ptr));
    let key0 = js_string_from_bytes("name".as_ptr(), 4);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key0));

    // Set type (as string for now)
    let type_name = col.type_info().name();
    let type_ptr = js_string_from_bytes(type_name.as_ptr(), type_name.len() as u32);
    js_object_set_field(obj, 1, JSValue::string_ptr(type_ptr));
    let key1 = js_string_from_bytes("type".as_ptr(), 4);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key1));

    // Set length (0 for now - would need to extract from column metadata)
    js_object_set_field(obj, 2, JSValue::number(0.0));
    let key2 = js_string_from_bytes("length".as_ptr(), 6);
    keys_array = js_array_push(keys_array, JSValue::string_ptr(key2));

    // Attach keys to object
    js_object_set_keys(obj, keys_array);

    obj
}
