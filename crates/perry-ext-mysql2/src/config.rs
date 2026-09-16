//! Connection parameters that are parsed rather than read off a field.
//!
//! `MySqlConfig`'s five scalars come straight off the JS object in
//! `crate::parse_mysql_config`. These do not: a `mysql://` URI has to be taken
//! apart and its credentials percent-decoded, and mysql2's `ssl` option arrives
//! in four different JS shapes. They share a file because they answer the same
//! question — what did the caller ask for — from the two places a caller can
//! say it, and they sit outside `lib.rs` because that file is at the 2,000-line
//! review cap.

use perry_ffi::{JsValue, ObjectHeader};

use crate::{jsvalue_to_string, object_field_by_name, MySqlConfig};

// ── The `mysql://` URI form ───────────────────────────────────────

/// Percent-decode a URI component (`%25` → `%`, `%40` → `@`, …). A lone `%`
/// not followed by two hex digits is kept verbatim. Node's `mysql2` decodes the
/// credentials it takes out of a connection URL, so a password written as
/// `p%25ss` (a literal `%`) authenticates as `p%ss`. Perry used the raw
/// substring and then RE-encoded it for sqlx, double-encoding every reserved
/// character — so a `%`/`@`/`:` in the password produced a wrong password and
/// the server rejected the connection with `1045 Access denied`. Decode here so
/// the round-trip through `to_url` reproduces the real credential.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn parse_mysql_uri(uri: &str) -> Option<MySqlConfig> {
    let uri = uri.strip_prefix("mysql://")?;
    let (credentials, host_part) = if let Some(idx) = uri.rfind('@') {
        (&uri[..idx], &uri[idx + 1..])
    } else {
        ("", uri)
    };
    let (user, password) = if let Some(idx) = credentials.find(':') {
        (
            percent_decode(&credentials[..idx]),
            percent_decode(&credentials[idx + 1..]),
        )
    } else {
        (percent_decode(credentials), String::new())
    };
    // The query string is libmysql's option surface and not part of the
    // database name. Splitting it off FIRST is the whole fix for a URI ending
    // `/app?ssl-mode=REQUIRED`: everything after the first `/` used to become
    // the database, so the connection asked for one literally called
    // `app?ssl-mode=REQUIRED` — which the server answers `Unknown database` —
    // and the request for TLS disappeared into the same string.
    let (location, query) = match host_part.find('?') {
        Some(idx) => (&host_part[..idx], &host_part[idx + 1..]),
        None => (host_part, ""),
    };
    let (host_port, database) = match location.find('/') {
        Some(idx) => {
            let name = &location[idx + 1..];
            // `mysql://host/?ssl-mode=REQUIRED` names no database. The empty
            // string is not a database the server has, so it must not be sent
            // as one.
            (
                &location[..idx],
                (!name.is_empty()).then(|| name.to_string()),
            )
        }
        None => (location, None),
    };
    let (host, port) = if let Some(idx) = host_port.rfind(':') {
        let port: u16 = host_port[idx + 1..].parse().unwrap_or(3306);
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
        ssl: ssl_from_uri_query(query),
    })
}

// ── The `ssl` option ──────────────────────────────────────────────

/// What mysql2's `ssl` option — or a URI's `ssl-mode` — asked for.
///
/// mysql2 accepts `ssl: true`, `ssl: "Amazon RDS"` and an options object; all
/// three mean the same thing on the wire (negotiate `CLIENT_SSL` in the
/// handshake response and refuse a server that does not offer it) and differ
/// only in the trust material they carry.
#[derive(Debug, Clone)]
pub struct MySqlSslConfig {
    /// Node's `rejectUnauthorized`.
    pub reject_unauthorized: bool,
    /// Explicit trust roots, PEM. Replaces the default set, as in Node.
    pub ca: Vec<u8>,
    /// Override the name verified and sent as SNI. Node calls it `servername`.
    pub servername: Option<String>,
}

impl Default for MySqlSslConfig {
    /// `reject_unauthorized` defaults to **true**, which is Node's default and
    /// not `bool`'s. A derived `Default` would make `ssl: {}` — an object that
    /// names no field at all — mean "encrypt, verify nothing", which is the one
    /// reading a caller who wrote `ssl: {}` cannot have intended.
    fn default() -> Self {
        Self {
            reject_unauthorized: true,
            ca: Vec::new(),
            servername: None,
        }
    }
}

/// libmysql's `ssl-mode` out of a connection URI's query string.
///
/// mysql2 itself reads no query string — it takes its options as an object —
/// but `mysql://…?ssl-mode=REQUIRED` is the form every MySQL CLI, DSN and
/// hosted provider hands out, and until now Perry swallowed the whole query
/// into the database name. Only the TLS part of libmysql's option surface is
/// implemented, so every other key is ignored rather than rejected.
fn ssl_from_uri_query(query: &str) -> Option<MySqlSslConfig> {
    let mut ssl = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if !key.eq_ignore_ascii_case("ssl-mode") && !key.eq_ignore_ascii_case("sslmode") {
            continue;
        }
        match value.trim().to_ascii_uppercase().as_str() {
            "DISABLED" => ssl = None,
            // `PREFERRED` is grouped with the three modes that require TLS
            // rather than with `DISABLED`: the core has no "try TLS, fall back
            // to plaintext" state — it either negotiates `CLIENT_SSL` or it
            // does not — and of the two readings, refusing a server that cannot
            // encrypt is the one that never sends the password in the clear.
            //
            // All four verify the certificate, which libmysql does only in its
            // two `VERIFY_*` modes. A connection string that asked for
            // encryption and then accepted any certificate would be a downgrade
            // nothing in it asked for; a caller who genuinely wants that says
            // `ssl: { rejectUnauthorized: false }`, where it is visible.
            "PREFERRED" | "REQUIRED" | "VERIFY_CA" | "VERIFY_IDENTITY" => {
                ssl = Some(MySqlSslConfig::default());
            }
            // A value libmysql would reject outright. Leaving the mode where it
            // was keeps a typo from deciding the question in either direction.
            _ => {}
        }
    }
    ssl
}

/// mysql2's `ssl`: absent/`false`, `true`, a profile name, or an options object.
///
/// Anything truthy that is not an object means "TLS with the default roots".
/// mysql2 spells one of those cases `ssl: "Amazon RDS"`, where the string names
/// a **bundled CA profile**; Perry bundles no profiles, so a string selects TLS
/// against the default root set rather than a profile. Every profile mysql2
/// ships is a public CA that is already in that set, and a caller with a
/// private root passes it as `ca`.
pub(crate) unsafe fn parse_mysql_ssl(value: JsValue) -> Option<MySqlSslConfig> {
    if value.is_undefined() || value.is_null() {
        return None;
    }
    if let Some(text) = jsvalue_to_string(value) {
        // `"disable"`/`"disabled"` is libmysql's spelling for off, and
        // `"false"` is what a string-typed environment variable arrives as.
        // Every other string — `"Amazon RDS"`, `"required"` — asks for TLS.
        if text.eq_ignore_ascii_case("disable")
            || text.eq_ignore_ascii_case("disabled")
            || text.eq_ignore_ascii_case("false")
        {
            return None;
        }
        return Some(MySqlSslConfig::default());
    }
    let mut ssl = MySqlSslConfig::default();
    if value.as_pointer::<ObjectHeader>().is_null() {
        // `ssl: true` — a boolean, with no fields to read.
        return value.to_bool().then_some(ssl);
    }
    // Read by name, through `crate::object_field_by_name`: it roots the
    // receiver across the key's `alloc_string`, which can move the object.
    let reject = object_field_by_name(value, "rejectUnauthorized");
    if !reject.is_undefined() && !reject.is_null() {
        ssl.reject_unauthorized = reject.to_bool();
    }
    if let Some(ca) = jsvalue_to_bytes(object_field_by_name(value, "ca")) {
        ssl.ca = ca;
    }
    if let Some(name) = jsvalue_to_string(object_field_by_name(value, "servername")) {
        ssl.servername = Some(name);
    }
    Some(ssl)
}

/// A `ca` may be a string or a Buffer — `fs.readFileSync` returns the latter.
///
/// The Buffer read goes through the **canonical runtime registry** rather than
/// perry-ffi's local one: this crate is a separately linked archive and cannot
/// see a Buffer the program runtime allocated. `perry-ext-net` learned the same
/// thing about `ca`/`cert`/`key` and its comment is the precedent.
unsafe fn jsvalue_to_bytes(value: JsValue) -> Option<Vec<u8>> {
    if let Some(text) = jsvalue_to_string(value) {
        return Some(text.into_bytes());
    }
    extern "C" {
        fn js_value_buffer_or_typedarray_data(value: f64, out_len: *mut u32) -> *const u8;
    }
    let mut len = 0u32;
    let data = js_value_buffer_or_typedarray_data(f64::from_bits(value.bits()), &mut len);
    if data.is_null() || len == 0 {
        None
    } else {
        Some(std::slice::from_raw_parts(data, len as usize).to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{
        alloc_buffer, alloc_string, build_object_shape, js_object_alloc_with_shape,
        js_object_set_field, TransientRootScope,
    };

    /// Build a JS object carrying `names`, each value produced by `value` once
    /// the object exists.
    ///
    /// The object is rooted across the value allocations rather than the other
    /// way round: `alloc_string` and `alloc_buffer` can move it, and a raw
    /// `*mut ObjectHeader` held across one is the #8217 shape the row builders
    /// in `lib.rs` are written to avoid.
    unsafe fn js_object(names: &[&str], value: impl Fn(usize) -> JsValue) -> JsValue {
        let (packed, shape_id) = build_object_shape(names);
        let roots = TransientRootScope::enter();
        let object = roots.root_nanbox(f64::from_bits(
            JsValue::from_object_ptr(js_object_alloc_with_shape(
                shape_id,
                names.len() as u32,
                packed.as_ptr(),
                packed.len() as u32,
            ))
            .bits(),
        ));
        for index in 0..names.len() {
            let field = value(index);
            let slot = JsValue::from_bits(object.get().to_bits()).as_pointer::<ObjectHeader>();
            js_object_set_field(slot, index as u32, field);
        }
        JsValue::from_bits(object.get().to_bits())
    }

    #[test]
    fn parse_uri_basic() {
        let p = parse_mysql_uri("mysql://root:secret@db.example.com:3307/mydb").unwrap();
        assert_eq!(p.host, "db.example.com");
        assert_eq!(p.port, 3307);
        assert_eq!(p.user, "root");
        assert_eq!(p.password, "secret");
        assert_eq!(p.database.as_deref(), Some("mydb"));
        assert!(p.ssl.is_none(), "a URI with no ssl-mode is plaintext");
    }

    #[test]
    fn percent_decode_credentials() {
        // Reserved characters in a percent-encoded password round-trip to the
        // literal value the server actually expects.
        assert_eq!(percent_decode("p%40ss"), "p@ss");
        assert_eq!(percent_decode("a%25b%2Fc%23"), "a%b/c#");
        assert_eq!(percent_decode("plain"), "plain");
        // A lone `%` (or one not followed by two hex digits) is kept verbatim.
        assert_eq!(percent_decode("50%off"), "50%off");
        assert_eq!(percent_decode("trailing%"), "trailing%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn parse_uri_percent_encoded_password() {
        // `@` inside the password is `%40`; the last `@` still splits creds/host.
        let p = parse_mysql_uri("mysql://user:p%40ss%2Fword@db.example.com/mydb").unwrap();
        assert_eq!(p.user, "user");
        assert_eq!(p.password, "p@ss/word");
        assert_eq!(p.host, "db.example.com");
    }

    #[test]
    fn a_uri_query_string_is_not_part_of_the_database_name() {
        // The defect: everything after the first `/` became the database, so
        // this URI connected to one literally called
        // `app?ssl-mode=REQUIRED&charset=utf8mb4` — `Unknown database` — and
        // the request for TLS vanished into the same string.
        let p = parse_mysql_uri(
            "mysql://root:secret@db.example.com:3307/app?ssl-mode=REQUIRED&charset=utf8mb4",
        )
        .expect("a mysql:// uri");
        assert_eq!(p.database.as_deref(), Some("app"));
        assert_eq!(p.host, "db.example.com");
        assert_eq!(p.port, 3307);
        assert!(p.ssl.is_some(), "?ssl-mode=REQUIRED asks for TLS");

        // A query string and no database: `None`, never `Some("")`, which the
        // server would answer `Unknown database ''`.
        let p = parse_mysql_uri("mysql://root@db.example.com/?sslmode=VERIFY_CA")
            .expect("a mysql:// uri");
        assert_eq!(p.database, None);
        assert!(p.ssl.is_some());

        // And with no `/` before the query at all, the port still parses.
        let p = parse_mysql_uri("mysql://root@db.example.com:3307?ssl-mode=DISABLED")
            .expect("a mysql:// uri");
        assert_eq!(p.host, "db.example.com");
        assert_eq!(p.port, 3307);
        assert_eq!(p.database, None);
        assert!(p.ssl.is_none());
    }

    #[test]
    fn a_uri_ssl_mode_is_read_with_libmysqls_own_vocabulary() {
        let tls = |uri: &str| parse_mysql_uri(uri).expect("a mysql:// uri").ssl.is_some();
        assert!(tls("mysql://root@h/db?ssl-mode=REQUIRED"));
        assert!(tls("mysql://root@h/db?ssl-mode=VERIFY_CA"));
        assert!(tls("mysql://root@h/db?ssl-mode=VERIFY_IDENTITY"));
        assert!(tls("mysql://root@h/db?ssl-mode=PREFERRED"));
        // The one-word spelling, and case-insensitively.
        assert!(tls("mysql://root@h/db?sslmode=required"));
        assert!(!tls("mysql://root@h/db?ssl-mode=DISABLED"));
        assert!(!tls("mysql://root@h/db"));
        // An unrecognised value decides nothing in either direction...
        assert!(!tls("mysql://root@h/db?ssl-mode=maybe"));
        // ...and the last `ssl-mode` in the string wins, as in a DSN.
        assert!(!tls(
            "mysql://root@h/db?ssl-mode=REQUIRED&ssl-mode=DISABLED"
        ));

        // Verification stays on. libmysql's `REQUIRED` does not verify; Perry
        // makes that an option a caller has to write down.
        let ssl = parse_mysql_uri("mysql://root@h/db?ssl-mode=REQUIRED")
            .unwrap()
            .ssl
            .unwrap();
        assert!(ssl.reject_unauthorized);
        assert!(ssl.ca.is_empty());
        assert_eq!(ssl.servername, None);
    }

    #[test]
    fn ssl_true_and_a_profile_name_select_tls_with_the_default_roots() {
        unsafe {
            let ssl = parse_mysql_ssl(JsValue::from_bool(true)).expect("`ssl: true` selects TLS");
            assert!(ssl.reject_unauthorized);
            assert!(ssl.ca.is_empty());

            // mysql2's bundled-CA-profile spelling. Perry bundles no profiles,
            // so the string selects TLS against the default roots.
            let name = alloc_string("Amazon RDS");
            let ssl = parse_mysql_ssl(JsValue::from_string_ptr(name.as_raw()))
                .expect("a profile name selects TLS");
            assert!(ssl.reject_unauthorized);
            assert!(ssl.ca.is_empty());
        }
    }

    #[test]
    fn ssl_absent_false_and_disabled_stay_plaintext() {
        unsafe {
            assert!(parse_mysql_ssl(JsValue::UNDEFINED).is_none());
            assert!(parse_mysql_ssl(JsValue::NULL).is_none());
            assert!(parse_mysql_ssl(JsValue::from_bool(false)).is_none());
            for text in ["disable", "DISABLED", "false"] {
                let value = alloc_string(text);
                assert!(
                    parse_mysql_ssl(JsValue::from_string_ptr(value.as_raw())).is_none(),
                    "`ssl: {text:?}` must stay plaintext"
                );
            }
        }
    }

    #[test]
    fn an_ssl_object_is_read_by_name_not_by_position() {
        unsafe {
            // Declared in an order no positional read would survive, and with a
            // field this parser does not know sitting first.
            let value = js_object(
                &["minVersion", "servername", "rejectUnauthorized"],
                |index| match index {
                    0 => JsValue::from_string_ptr(alloc_string("TLSv1.2").as_raw()),
                    1 => JsValue::from_string_ptr(alloc_string("db.internal").as_raw()),
                    _ => JsValue::from_bool(false),
                },
            );
            let ssl = parse_mysql_ssl(value).expect("an ssl object selects TLS");
            assert!(
                !ssl.reject_unauthorized,
                "rejectUnauthorized: false must reach the session"
            );
            assert_eq!(ssl.servername.as_deref(), Some("db.internal"));
            assert!(ssl.ca.is_empty());
        }
    }

    #[test]
    fn an_ssl_ca_accepts_the_buffer_readfilesync_returns() {
        unsafe {
            const PEM: &[u8] = b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
            let value = js_object(&["ca"], |_| JsValue::from_object_ptr(alloc_buffer(PEM)));
            let ssl = parse_mysql_ssl(value).expect("an ssl object selects TLS");
            assert_eq!(ssl.ca, PEM.to_vec());
            // An object that names no `rejectUnauthorized` keeps Node's default.
            assert!(ssl.reject_unauthorized);

            // The same material as a string, which is what an inlined PEM is.
            let value = js_object(&["ca"], |_| {
                JsValue::from_string_ptr(alloc_string(std::str::from_utf8(PEM).unwrap()).as_raw())
            });
            let ssl = parse_mysql_ssl(value).expect("an ssl object selects TLS");
            assert_eq!(ssl.ca, PEM.to_vec());
        }
    }
}
