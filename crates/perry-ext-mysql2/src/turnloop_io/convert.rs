//! Wire values → the crate's owned [`crate::RawValue`], reproducing the sqlx
//! path's conversion policy exactly.
//!
//! # Why this is hand-written and not `turnloop_mysql::types::decode`
//!
//! The crate ships a default mysql2 policy, but Perry's shipped policy is not
//! it, and the differences are observable from JS:
//!
//! | column | `types::decode` default | Perry's sqlx path (what this reproduces) |
//! |---|---|---|
//! | DECIMAL | exact string | `f64` |
//! | DATETIME | `Date` with microseconds | `"%Y-%m-%d %H:%M:%S"`, seconds truncated |
//! | BLOB / BINARY | `Buffer` bytes | lossy-UTF-8 **string** |
//! | TIME | string, negative and >24 h preserved | `chrono::NaiveTime`, so out-of-range is `null` |
//!
//! Rewriting those as option flags would still leave the microsecond and the
//! out-of-range-TIME cases wrong, so the mapping is spelled out here where it
//! can be read against `crate::extract_raw_value` line for line.
//!
//! The two deliberate departures are named at their arms: YEAR, which the sqlx
//! path decoded as `null`, and the type id for the small BLOB/TEXT families.
//! Both are called out in the tests below.

use turnloop_mysql::{ColumnFlags, ColumnType, ColumnTypeInfo, RawValue as WireValue, Row, Value};

use crate::{RawColumnInfo, RawRowData, RawValue};

/// One result-set column, owned. The wire `Column<'a>` borrows the receive
/// buffer and dies at the next mutable call on the core, so a column is copied
/// out the moment its event is drained.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OwnedColumn {
    pub(crate) name: String,
    pub(crate) info: ColumnTypeInfo,
}

impl OwnedColumn {
    /// The field-packet description the existing result builder consumes.
    ///
    /// `type_name` is sqlx's spelling rather than the wire id because
    /// `crate::raw_column_to_field_packet` maps the name back to the id with
    /// `crate::mysql_type_id_from_name`; keeping that one mapping means the
    /// `[rows, fields]` tuple is still built by the same code on both
    /// transports, key order included.
    pub(crate) fn describe(&self) -> RawColumnInfo {
        RawColumnInfo {
            name: self.name.clone(),
            type_name: sqlx_type_name(self.info).to_string(),
        }
    }
}

/// sqlx's `MySqlTypeInfo::name()` spelling for a wire column.
///
/// Only two things depend on it: the numeric `field.type` the result builder
/// derives, and readability against the sqlx arm it replaces.
///
/// One named divergence: MySQL reports every BLOB/TEXT size class as the same
/// wire type (252), so `TINYTEXT` and `LONGBLOB` are indistinguishable here and
/// both come back as `BLOB`/`TEXT` → type id 252. sqlx recovered the size class
/// from the column's `max_size` and could answer 249/250/251. 252 is what
/// Node's mysql2 reports for all four, so this moves *towards* Node, but it is
/// a change in a resolved value and is pinned by a test.
pub(crate) fn sqlx_type_name(info: ColumnTypeInfo) -> &'static str {
    let unsigned = info.flags.contains(ColumnFlags::UNSIGNED_FLAG);
    let binary = info.character_set == 63;
    match info.column_type {
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        ColumnType::MYSQL_TYPE_TINY => {
            if unsigned {
                "TINYINT UNSIGNED"
            } else {
                "TINYINT"
            }
        }
        ColumnType::MYSQL_TYPE_SHORT => {
            if unsigned {
                "SMALLINT UNSIGNED"
            } else {
                "SMALLINT"
            }
        }
        ColumnType::MYSQL_TYPE_INT24 => {
            if unsigned {
                "MEDIUMINT UNSIGNED"
            } else {
                "MEDIUMINT"
            }
        }
        ColumnType::MYSQL_TYPE_LONG => {
            if unsigned {
                "INT UNSIGNED"
            } else {
                "INT"
            }
        }
        ColumnType::MYSQL_TYPE_LONGLONG => {
            if unsigned {
                "BIGINT UNSIGNED"
            } else {
                "BIGINT"
            }
        }
        ColumnType::MYSQL_TYPE_FLOAT => "FLOAT",
        ColumnType::MYSQL_TYPE_DOUBLE => "DOUBLE",
        ColumnType::MYSQL_TYPE_NULL => "NULL",
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "DATE",
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "TIME",
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "DATETIME",
        ColumnType::MYSQL_TYPE_YEAR => "YEAR",
        ColumnType::MYSQL_TYPE_BIT => "BIT",
        ColumnType::MYSQL_TYPE_JSON => "JSON",
        ColumnType::MYSQL_TYPE_ENUM => "ENUM",
        ColumnType::MYSQL_TYPE_SET => "SET",
        ColumnType::MYSQL_TYPE_TINY_BLOB => {
            if binary {
                "TINYBLOB"
            } else {
                "TINYTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_MEDIUM_BLOB => {
            if binary {
                "MEDIUMBLOB"
            } else {
                "MEDIUMTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_LONG_BLOB => {
            if binary {
                "LONGBLOB"
            } else {
                "LONGTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_BLOB => {
            if binary {
                "BLOB"
            } else {
                "TEXT"
            }
        }
        ColumnType::MYSQL_TYPE_VARCHAR | ColumnType::MYSQL_TYPE_VAR_STRING => {
            if binary {
                "VARBINARY"
            } else {
                "VARCHAR"
            }
        }
        ColumnType::MYSQL_TYPE_STRING => {
            if binary {
                "BINARY"
            } else {
                "CHAR"
            }
        }
        ColumnType::MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        // VECTOR / UNKNOWN / TYPED_ARRAY have no sqlx name and no mysql2 id;
        // the result builder answers 0 for an unknown name, which is what the
        // sqlx path did for anything it could not name either.
        _ => "",
    }
}

/// Copy one wire row out of the receive buffer.
///
/// Called while draining, before any further mutable call on the core: `Row`
/// borrows the packet bytes and `RawValue::Bytes` points straight into them.
pub(crate) fn decode_row(row: Row<'_>, columns: &[OwnedColumn]) -> RawRowData {
    let mut values = Vec::with_capacity(columns.len());
    for (column, cell) in columns.iter().zip(row) {
        let decoded = match cell {
            Ok(raw) => decode(column.info, raw),
            // A cell the core could not parse is `null` rather than a failed
            // row: `try_get` in the sqlx path did the same, and rejecting the
            // whole query for one unreadable cell would be a new failure mode.
            Err(_) => RawValue::Null,
        };
        values.push((column.name.clone(), decoded));
    }
    RawRowData { values }
}

/// One cell. Mirrors `crate::extract_raw_value`, arm for arm.
pub(crate) fn decode(info: ColumnTypeInfo, raw: WireValue<'_>) -> RawValue {
    use ColumnType::*;
    if matches!(raw, WireValue::Null) {
        return RawValue::Null;
    }
    match info.column_type {
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => number(raw),
        // TINYINT(1) included: the sqlx path's `BOOLEAN`/`BOOL` arm could not
        // fire, because sqlx names MySQL's one-byte integer `TINYINT`
        // regardless of its display width. Node's mysql2 also answers a number
        // here unless you opt into its boolean cast.
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG
        | MYSQL_TYPE_LONGLONG | MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE => number(raw),
        // A deliberate departure: the sqlx path had no `YEAR` arm, so YEAR fell
        // into its catch-all, where neither `String` nor `Vec<u8>` is a legal
        // sqlx decode target for it — every YEAR column read back as `null`.
        // Answering the number is what Node's mysql2 does.
        MYSQL_TYPE_YEAR => number(raw),
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => match calendar(raw) {
            Some((y, m, d, _, _, _)) => {
                match chrono::NaiveDate::from_ymd_opt(i32::from(y), u32::from(m), u32::from(d)) {
                    Some(date) => RawValue::String(date.format("%Y-%m-%d").to_string()),
                    // MySQL's zero date (`0000-00-00`) is not a `chrono`
                    // date, so sqlx's `try_get` failed and the cell read
                    // `null`. Keep that: a program that stores zero dates is
                    // already seeing `null` today.
                    None => RawValue::Null,
                }
            }
            None => RawValue::Null,
        },
        MYSQL_TYPE_DATETIME
        | MYSQL_TYPE_DATETIME2
        | MYSQL_TYPE_TIMESTAMP
        | MYSQL_TYPE_TIMESTAMP2 => match calendar(raw) {
            Some((y, mo, d, h, mi, s)) => {
                chrono::NaiveDate::from_ymd_opt(i32::from(y), u32::from(mo), u32::from(d))
                    .and_then(|date| date.and_hms_opt(u32::from(h), u32::from(mi), u32::from(s)))
                    .map(|at| RawValue::String(at.format("%Y-%m-%d %H:%M:%S").to_string()))
                    // Sub-second precision is dropped, as it was under sqlx's
                    // `NaiveDateTime` + `%H:%M:%S` format string. A DATETIME(6)
                    // therefore still answers whole seconds.
                    .unwrap_or(RawValue::Null)
            }
            None => RawValue::Null,
        },
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => match clock(raw) {
            Some((negative, hours, minutes, seconds)) => {
                // MySQL's TIME spans -838:59:59..=838:59:59, which
                // `chrono::NaiveTime` cannot hold; sqlx's decode failed for
                // those and the cell read `null`. Reproduced rather than fixed,
                // so no program's values move on this change. Node's mysql2
                // would answer the string.
                if negative {
                    RawValue::Null
                } else {
                    match chrono::NaiveTime::from_hms_opt(
                        hours,
                        u32::from(minutes),
                        u32::from(seconds),
                    ) {
                        Some(time) => RawValue::String(time.format("%H:%M:%S").to_string()),
                        None => RawValue::Null,
                    }
                }
            }
            None => RawValue::Null,
        },
        MYSQL_TYPE_JSON => match bytes(&raw) {
            // Parsed here, not handed over as text: Node's mysql2 gives back
            // the parsed document and drizzle's `json()` mapper relies on it.
            // A document the parser rejects reads `null`, which is what
            // `try_get::<serde_json::Value>` did.
            Some(b) => serde_json::from_slice::<serde_json::Value>(b)
                .map(RawValue::Json)
                .unwrap_or(RawValue::Null),
            None => RawValue::Null,
        },
        // Everything else is the sqlx catch-all: `String`, then `Vec<u8>`
        // lossily. BLOB and BINARY columns therefore come back as **strings**,
        // not Buffers — a known divergence from Node's mysql2 that this change
        // deliberately does not move, because programs are reading those
        // strings today.
        _ => match bytes(&raw) {
            Some(b) => RawValue::String(String::from_utf8_lossy(b).into_owned()),
            None => RawValue::Null,
        },
    }
}

/// The numeric reading of a cell, from either protocol.
///
/// The text protocol sends every number as ASCII; the binary protocol sends a
/// typed scalar. Both end as an `f64`, which is the only numeric shape the
/// result builder has.
fn number(raw: WireValue<'_>) -> RawValue {
    match raw {
        WireValue::Null => RawValue::Null,
        WireValue::Bytes(b) => std::str::from_utf8(b)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(RawValue::Float64)
            .unwrap_or(RawValue::Null),
        WireValue::Scalar(Value::Int(n)) => RawValue::Float64(n as f64),
        WireValue::Scalar(Value::UInt(n)) => RawValue::Float64(n as f64),
        WireValue::Scalar(Value::Float(n)) => RawValue::Float64(f64::from(n)),
        WireValue::Scalar(Value::Double(n)) => RawValue::Float64(n),
        _ => RawValue::Null,
    }
}

/// `(year, month, day, hour, minute, second)` from either protocol.
///
/// Microseconds are read and discarded: see the DATETIME arm.
fn calendar(raw: WireValue<'_>) -> Option<(u16, u8, u8, u8, u8, u8)> {
    match raw {
        WireValue::Scalar(Value::Date(y, mo, d, h, mi, s, _)) => Some((y, mo, d, h, mi, s)),
        WireValue::Bytes(b) => parse_calendar(std::str::from_utf8(b).ok()?),
        _ => None,
    }
}

/// `YYYY-MM-DD[ HH:MM:SS[.ffffff]]`, the text protocol's temporal spelling.
fn parse_calendar(text: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
    let (date, time) = match text.split_once(' ') {
        Some((date, time)) => (date, Some(time)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let (hour, minute, second) = match time {
        Some(time) => {
            let time = time.split('.').next().unwrap_or(time);
            let mut parts = time.split(':');
            let hour = parts.next()?.parse().ok()?;
            let minute = parts.next()?.parse().ok()?;
            let second = parts.next()?.parse().ok()?;
            (hour, minute, second)
        }
        None => (0, 0, 0),
    };
    Some((year, month, day, hour, minute, second))
}

/// `(negative, hours, minutes, seconds)` for a TIME cell, from either protocol.
///
/// `hours` is a `u32` and not a `u8` because MySQL's TIME carries whole days:
/// the binary form is `(negative, days, hours, …)` and `838:59:59` is legal.
fn clock(raw: WireValue<'_>) -> Option<(bool, u32, u8, u8)> {
    match raw {
        WireValue::Scalar(Value::Time(negative, days, hours, minutes, seconds, _)) => {
            Some((negative, days * 24 + u32::from(hours), minutes, seconds))
        }
        WireValue::Bytes(b) => {
            let text = std::str::from_utf8(b).ok()?;
            let (negative, text) = match text.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, text),
            };
            let text = text.split('.').next().unwrap_or(text);
            let mut parts = text.split(':');
            let hours: u32 = parts.next()?.parse().ok()?;
            let minutes: u8 = parts.next()?.parse().ok()?;
            let seconds: u8 = parts.next()?.parse().ok()?;
            Some((negative, hours, minutes, seconds))
        }
        _ => None,
    }
}

/// The raw bytes behind a cell, when it has any.
///
/// A binary-protocol scalar never carries bytes — `turnloop_mysql` hands text
/// and blob columns back as `Bytes` in both protocols — so `None` here means a
/// numeric scalar arrived for a column this arm does not expect.
fn bytes<'a>(raw: &WireValue<'a>) -> Option<&'a [u8]> {
    match raw {
        WireValue::Bytes(b) => Some(b),
        _ => None,
    }
}

/// Bind values, translated for `Connection::execute`.
///
/// One-for-one with the sqlx `query.bind(..)` chain it replaces; the only
/// judgement call is `Bool`, which sqlx encoded as MySQL's TINYINT 0/1.
pub(crate) fn bind_values(params: &[crate::ParamValue]) -> Vec<Value> {
    params
        .iter()
        .map(|param| match param {
            crate::ParamValue::Null => Value::NULL,
            crate::ParamValue::String(s) => Value::Bytes(s.clone().into_bytes()),
            crate::ParamValue::Bytes(b) => Value::Bytes(b.clone()),
            crate::ParamValue::DateTime(at) => {
                use chrono::{Datelike, Timelike};
                Value::Date(
                    at.year().clamp(0, i32::from(u16::MAX)) as u16,
                    at.month() as u8,
                    at.day() as u8,
                    at.hour() as u8,
                    at.minute() as u8,
                    at.second() as u8,
                    at.and_utc().timestamp_subsec_micros(),
                )
            }
            crate::ParamValue::Number(n) => Value::Double(*n),
            crate::ParamValue::Int(i) => Value::Int(*i),
            crate::ParamValue::Bool(b) => Value::Int(i64::from(*b)),
        })
        .collect()
}
