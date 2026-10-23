// Adapted from Wasmtime 48.0.5 wasi-http/src/field_map.rs.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

use super::forbidden_header;
use http::header::Entry;
use http::{HeaderMap, HeaderName, HeaderValue};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use wasmtime::Result;

/// A wrapper around [`http::HeaderMap`] which implements `wasi:http` semantics.
///
/// The main differences from [`http::HeaderMap`] and this type are:
///
/// * A slimmed down mutability API to just what `wasi:http` needs.
/// * `FieldMap` is cheaply clone-able with the internal `HeaderMap` being
///   behind an `Arc`.
/// * `FieldMap` is either immutable or mutable. Mutations on immutable values
///   are rejected with an error. Mutations on mutable values will never panic
///   unlike `HeaderMap` and additionally require a limit to be set on the size
///   of the map.
///
/// Overall the intention is that this is a slim wrapper around
/// [`http::HeaderMap`] with slightly different ownership, panic, and error
/// semantics.
#[derive(Debug, Clone)]
pub struct FieldMap {
    map: Arc<HeaderMap>,
    ordered: Arc<Vec<(String, HeaderValue)>>,
    limit: Limit,
    size: usize,
}

#[derive(Debug, Clone)]
enum Limit {
    Mutable(usize),
    Immutable,
}

impl Default for FieldMap {
    fn default() -> Self {
        Self {
            map: Arc::new(HeaderMap::new()),
            ordered: Arc::new(Vec::new()),
            size: 0,
            limit: Limit::Immutable,
        }
    }
}

impl FieldMap {
    /// Creates a new immutable `FieldMap` from the provided
    /// [`http::HeaderMap`].
    ///
    /// The returned value cannot be mutated and attempting to mutate it will
    /// return an error.
    pub fn new_immutable(mut map: HeaderMap) -> Self {
        // Strip out all forbidden headers from `map` to ensure they're never
        // able to enter into a WASI guest.
        let forbidden_keys = Vec::from_iter(map.keys().filter_map(|name| {
            if forbidden_header(name) {
                Some(name.clone())
            } else {
                None
            }
        }));
        for name in forbidden_keys {
            map.remove(&name);
        }

        let ordered = map
            .iter()
            .map(|(n, v)| (n.as_str().to_owned(), v.clone()))
            .collect::<Vec<_>>();
        let size = Self::content_size(&map) + ordered_size(&ordered);
        Self {
            map: Arc::new(map),
            ordered: Arc::new(ordered),
            size,
            limit: Limit::Immutable,
        }
    }

    /// Construct received fields without losing their spelling or wire order.
    pub(crate) fn from_received(
        entries: Vec<(String, HeaderValue)>,
    ) -> Result<Self, FieldMapError> {
        let mut map = HeaderMap::new();
        let mut ordered = Vec::new();
        for (name, value) in entries {
            let key: HeaderName = name.parse()?;
            if forbidden_header(&key) {
                continue;
            }
            map.try_append(key, value.clone())?;
            ordered.push((name, value));
        }
        Ok(Self {
            size: Self::content_size(&map) + ordered_size(&ordered),
            map: Arc::new(map),
            ordered: Arc::new(ordered),
            limit: Limit::Immutable,
        })
    }

    /// Original spelling and insertion order, shared by copy-all and transport.
    pub fn ordered_iter(&self) -> impl Iterator<Item = (&str, &HeaderValue)> {
        self.ordered.iter().map(|(n, v)| (n.as_str(), v))
    }

    /// Creates a new, empty, mutable `FieldMap`.
    ///
    /// Mutations are allowed on the returned value and up to `limit` bytes of
    /// memory (roughly) may be consumed by this map.
    pub fn new_mutable(limit: usize) -> Self {
        Self {
            map: Arc::new(HeaderMap::new()),
            ordered: Arc::new(Vec::new()),
            size: 0,
            limit: Limit::Mutable(limit),
        }
    }

    /// Calculate the content size of a `HeaderMap`. This is a sum of the size
    /// of all of the keys and all of the values.
    pub(crate) fn content_size(map: &HeaderMap) -> usize {
        let mut sum = 0;
        for key in map.keys() {
            sum += header_name_size(key);
        }
        for value in map.values() {
            sum += header_value_size(value);
        }
        sum
    }

    /// Sets the header `key` to the `values` list provided.
    ///
    /// Removes the previous value, if any.
    ///
    /// If `values` is empty then this removes the header `key`.
    pub fn set(&mut self, key: String, values: Vec<Vec<u8>>) -> Result<(), FieldMapError> {
        let original = key;
        let key: HeaderName = original.parse()?;
        if forbidden_header(&key) {
            return Err(FieldMapError::Forbidden);
        }
        let limit = match self.limit {
            Limit::Immutable => return Err(FieldMapError::Immutable),
            Limit::Mutable(limit) => limit,
        };
        let values = values
            .into_iter()
            .map(|v| parse_header_value(&key, v))
            .collect::<Result<Vec<_>, _>>()?;
        let previous = self
            .map
            .get_all(&key)
            .iter()
            .map(header_value_size)
            .sum::<usize>();
        let previous_key = if self.map.contains_key(&key) {
            header_name_size(&key)
        } else {
            0
        };
        let next_key = if values.is_empty() {
            0
        } else {
            header_name_size(&key)
        };
        let removed_ordered = self
            .ordered
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(&original))
            .map(|(n, _)| ordered_entry_size(n))
            .sum::<usize>();
        let added_ordered = values.len() * ordered_entry_size(&original);
        let next = self.size - previous - previous_key - removed_ordered
            + added_ordered
            + next_key
            + values.iter().map(header_value_size).sum::<usize>();
        check_size(limit, next)?;
        // Mutate a candidate so HeaderMap capacity errors cannot leave either
        // the original values or their accounting partially changed.
        let mut candidate = (*self.map).clone();
        candidate.remove(&key);
        candidate.try_reserve(values.len())?;
        for value in &values {
            candidate.try_append(key.clone(), value.clone())?;
        }
        let ordered = Arc::make_mut(&mut self.ordered);
        ordered.retain(|(n, _)| !n.eq_ignore_ascii_case(&original));
        ordered.extend(values.into_iter().map(|v| (original.clone(), v)));
        self.map = Arc::new(candidate);
        self.size = next;
        Ok(())
    }

    /// Remove all values associated with a key in a map.
    ///
    /// Returns an empty list if the key is not already present within the map.
    pub fn remove_all(&mut self, key: String) -> Result<Vec<HeaderValue>, FieldMapError> {
        let original = key;
        let key: HeaderName = original.parse()?;
        if forbidden_header(&key) {
            return Err(FieldMapError::Forbidden);
        }
        let (map, _limit, size) = self.mutable()?;
        match map.try_entry(key)? {
            Entry::Vacant { .. } => Ok(Vec::new()),
            Entry::Occupied(e) => {
                let (name, value_drain) = e.remove_entry_mult();
                let mut removed = header_name_size(&name);
                let values = value_drain.collect::<Vec<_>>();
                for v in values.iter() {
                    removed += header_value_size(v);
                }
                *size -= removed;
                let ordered = Arc::make_mut(&mut self.ordered);
                let removed = ordered
                    .iter()
                    .filter(|(n, _)| n.eq_ignore_ascii_case(&original))
                    .map(|(n, _)| ordered_entry_size(n))
                    .sum::<usize>();
                ordered.retain(|(n, _)| !n.eq_ignore_ascii_case(&original));
                self.size -= removed;
                Ok(values)
            }
        }
    }

    fn mutable(&mut self) -> Result<(&mut HeaderMap, usize, &mut usize), FieldMapError> {
        match self.limit {
            Limit::Immutable => Err(FieldMapError::Immutable),
            Limit::Mutable(limit) => Ok((Arc::make_mut(&mut self.map), limit, &mut self.size)),
        }
    }

    /// Add a value associated with a key to the map.
    ///
    /// If `key` is already present within the map then `value` is appended to
    /// the list of values it already has.
    pub fn append(&mut self, key: String, value: Vec<u8>) -> Result<bool, FieldMapError> {
        let original = key;
        let key: HeaderName = original.parse()?;
        if forbidden_header(&key) {
            return Err(FieldMapError::Forbidden);
        }
        let value = parse_header_value(&key, value)?;
        let result = self.append_raw(key, value.clone(), ordered_entry_size(&original))?;
        Arc::make_mut(&mut self.ordered).push((original, value));
        Ok(result)
    }

    /// Add a value associated with a key to the map.
    ///
    /// If `key` is already present within the map then `value` is appended to
    /// the list of values it already has.
    fn append_raw(
        &mut self,
        key: HeaderName,
        value: HeaderValue,
        ordered_cost: usize,
    ) -> Result<bool, FieldMapError> {
        let (map, limit, size) = self.mutable()?;
        let key_size = header_name_size(&key);
        let val_size = header_value_size(&value);
        let new_size = if !map.contains_key(&key) {
            *size + key_size + val_size
        } else {
            *size + val_size
        };
        let new_size = new_size + ordered_cost;
        check_size(limit, new_size)?;
        let already_present = map.try_append(key, value)?;
        *size = new_size;
        Ok(already_present)
    }

    /// Flags this map as mutable, allowing mutations which can allocate as much
    /// as `limit` memory, in bytes, for this entire map (roughly).
    pub fn set_mutable(&mut self, limit: usize) {
        self.limit = Limit::Mutable(limit);
    }

    /// Flags this map as immutable, forbidding all further mutations.
    pub fn set_immutable(&mut self) {
        self.limit = Limit::Immutable;
    }
}

// Include the ordered representation in the limit, including repeated names.
fn ordered_entry_size(name: &str) -> usize {
    name.len() + size_of::<(String, HeaderValue)>()
}
fn ordered_size(entries: &[(String, HeaderValue)]) -> usize {
    entries.iter().map(|(n, _)| ordered_entry_size(n)).sum()
}

/// Returns the size, in accounting cost, to consider for `name`.
///
/// This includes both the byte length of the `name` itself as well as the size
/// of the data structure itself as it'll reside within a `HeaderMap`.
fn header_name_size(name: &HeaderName) -> usize {
    name.as_str().len() + size_of::<HeaderName>()
}

/// Same as `header_name_size`, but for values.
///
/// This notably includes the size of `HeaderValue` itself to ensure that all
/// headers have a nonzero size as otherwise this would never limit addition of
/// an empty header value.
fn header_value_size(value: &HeaderValue) -> usize {
    value.len() + size_of::<HeaderValue>()
}

fn check_size(limit: usize, new: usize) -> Result<(), FieldMapError> {
    if new > limit {
        Err(FieldMapError::TotalSizeTooBig)
    } else {
        Ok(())
    }
}

// Note that `DerefMut` is specifically omitted here to force all mutations
// through the `FieldMap` wrapper.
impl Deref for FieldMap {
    type Target = HeaderMap;

    fn deref(&self) -> &HeaderMap {
        &self.map
    }
}

impl From<FieldMap> for HeaderMap {
    fn from(map: FieldMap) -> Self {
        Arc::unwrap_or_clone(map.map)
    }
}

/// Errors that can happen when mutating/operating on a [`FieldMap`].
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FieldMapError {
    /// A mutation was attempted when the map is not mutable.
    Immutable,
    /// The map has too many fields and is not allowed to add more.
    ///
    /// Note that this is currently a limitation inherited from
    /// [`http::HeaderMap`].
    TooManyFields,
    /// The map's total size, of keys and values, is too large.
    TotalSizeTooBig,
    /// An invalid header name was attempted to be added.
    InvalidHeaderName,
    /// An invalid header value was attempted to be added.
    InvalidHeaderValue,
    /// A forbidden header name was used.
    Forbidden,
}

impl fmt::Display for FieldMapError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            FieldMapError::Immutable => "cannot mutate an immutable field map",
            FieldMapError::TooManyFields => "too many fields in the field map",
            FieldMapError::TotalSizeTooBig => "total size of fields exceeds limit",
            FieldMapError::InvalidHeaderName => "invalid header name",
            FieldMapError::InvalidHeaderValue => "invalid header value",
            FieldMapError::Forbidden => "forbidden header name",
        };
        f.write_str(s)
    }
}

impl std::error::Error for FieldMapError {}

impl From<http::header::MaxSizeReached> for FieldMapError {
    fn from(_: http::header::MaxSizeReached) -> Self {
        Self::TooManyFields
    }
}

impl From<http::header::InvalidHeaderName> for FieldMapError {
    fn from(_: http::header::InvalidHeaderName) -> Self {
        Self::InvalidHeaderName
    }
}

impl From<http::header::InvalidHeaderValue> for FieldMapError {
    fn from(_: http::header::InvalidHeaderValue) -> Self {
        Self::InvalidHeaderValue
    }
}

fn parse_header_value(
    name: &http::HeaderName,
    value: Vec<u8>,
) -> Result<http::HeaderValue, FieldMapError> {
    if name == http::header::CONTENT_LENGTH {
        let s = str::from_utf8(value.as_ref()).or(Err(FieldMapError::InvalidHeaderValue))?;
        // RFC 9110 defines `Content-Length` as `1*DIGIT`. `u64`'s `FromStr` is
        // more lenient and also accepts a leading `+`, so reject anything that
        // isn't a non-empty run of decimal digits.
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(FieldMapError::InvalidHeaderValue);
        }
        let v: u64 = s.parse().or(Err(FieldMapError::InvalidHeaderValue))?;
        Ok(v.into())
    } else {
        let value = value.try_into()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_header_value, FieldMap, FieldMapError};
    use http::header::{CONTENT_LENGTH, CONTENT_TYPE};

    #[test]
    fn original_names_order_and_failed_mutations_survive_snapshots() {
        let mut map = FieldMap::new_mutable(2048);
        for (n, v) in [("X-A", "first"), ("X-B", "middle"), ("x-a", "last")] {
            map.append(n.into(), v.as_bytes().to_vec()).unwrap();
        }
        let snapshot = map.clone();
        let entries = |m: &FieldMap| {
            m.ordered_iter()
                .map(|(n, v)| (n.to_owned(), v.to_str().unwrap().to_owned()))
                .collect::<Vec<_>>()
        };
        let original = vec![
            ("X-A".into(), "first".into()),
            ("X-B".into(), "middle".into()),
            ("x-a".into(), "last".into()),
        ];
        assert_eq!(entries(&map), original);
        assert_eq!(
            map.set("x-A".into(), vec![vec![b'x'; 2048]]),
            Err(FieldMapError::TotalSizeTooBig)
        );
        assert_eq!(entries(&map), original);
        map.set("x-A".into(), vec![b"new".to_vec(), b"second".to_vec()])
            .unwrap();
        assert_eq!(
            entries(&map),
            vec![
                ("X-B".into(), "middle".into()),
                ("x-A".into(), "new".into()),
                ("x-A".into(), "second".into())
            ]
        );
        assert_eq!(entries(&snapshot), original);
        map.remove_all("X-a".into()).unwrap();
        assert_eq!(entries(&map), vec![("X-B".into(), "middle".into())]);
        map.remove_all("x-b".into()).unwrap();
        assert_eq!(map.size, 0);
        let received = FieldMap::from_received(
            snapshot
                .ordered_iter()
                .map(|(n, v)| (n.to_owned(), v.clone()))
                .chain(std::iter::once(("Host".into(), "hidden".parse().unwrap())))
                .collect(),
        )
        .unwrap();
        assert_eq!(entries(&received), original);
        assert!(!received.contains_key("host"));
    }

    #[test]
    fn test_immutable() {
        let mut map = FieldMap::default();
        assert_eq!(
            map.set("foo".to_owned(), vec![b"bar".to_vec()]),
            Err(FieldMapError::Immutable)
        );
        assert_eq!(
            map.append("foo".to_owned(), b"bar".to_vec()),
            Err(FieldMapError::Immutable)
        );
        assert_eq!(
            map.remove_all("foo".to_owned()),
            Err(FieldMapError::Immutable)
        );
    }

    #[test]
    fn test_limits() {
        let mut map = FieldMap::new_mutable(100);
        loop {
            match map.append("foo".to_owned(), b"bar".to_vec()) {
                Ok(_) => {}
                Err(FieldMapError::TotalSizeTooBig) => break,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        map = FieldMap::new_mutable(100);
        for i in 0.. {
            match map.set(
                "foo".to_owned(),
                (0..i).map(|j| format!("bar{j}").into_bytes()).collect(),
            ) {
                Ok(_) => {}
                Err(FieldMapError::TotalSizeTooBig) => break,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        map = FieldMap::new_mutable(100);
        for i in 0.. {
            match map.set(format!("foo{i}"), vec![b"bar".to_vec()]) {
                Ok(_) => {}
                Err(FieldMapError::TotalSizeTooBig) => break,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }
    }

    #[test]
    fn test_size() -> Result<(), FieldMapError> {
        let mut map = FieldMap::new_mutable(2000);
        let name = "foo".to_owned();

        map.append(name.clone(), b"bar".to_vec())?;
        assert!(map.size > 0);
        map.remove_all(name.clone())?;
        assert_eq!(map.size, 0);

        map.set(name.clone(), vec![b"bar".to_vec()])?;
        assert!(map.size > 0);
        map.remove_all(name.clone())?;
        assert_eq!(map.size, 0);

        map.set(name.clone(), vec![])?;
        assert_eq!(map.size, 0);
        map.set(name.clone(), vec![b"bar".to_vec()])?;
        assert!(map.size > 0);
        map.set(name.clone(), vec![])?;
        assert_eq!(map.size, 0);

        map.set(name.clone(), vec![b"bar".to_vec()])?;
        assert!(map.size > 0);
        map.set(name.clone(), vec![b"bar".to_vec(), b"baz".to_vec()])?;
        assert!(map.size > 0);
        map.remove_all(name.clone())?;
        assert_eq!(map.size, 0);

        Ok(())
    }

    #[test]
    fn content_length_rejects_non_digits() {
        assert!(parse_header_value(&CONTENT_LENGTH, b"0".to_vec()).is_ok());
        assert!(parse_header_value(&CONTENT_LENGTH, b"1234".to_vec()).is_ok());

        // `u64::from_str` accepts these but they are not `1*DIGIT` per RFC 9110.
        assert!(parse_header_value(&CONTENT_LENGTH, b"+5".to_vec()).is_err());
        assert!(parse_header_value(&CONTENT_LENGTH, b"-5".to_vec()).is_err());
        assert!(parse_header_value(&CONTENT_LENGTH, b" 5".to_vec()).is_err());
        assert!(parse_header_value(&CONTENT_LENGTH, b"".to_vec()).is_err());

        // other header names are unaffected
        assert!(parse_header_value(&CONTENT_TYPE, b"text/plain".to_vec()).is_ok());
    }
    #[test]
    fn failed_mutations_leave_values_and_accounting_unchanged() {
        let mut map = FieldMap::new_mutable(160);
        map.set("x-test".into(), vec![b"kept".to_vec()]).unwrap();
        let size = map.size;
        assert_eq!(
            map.set("x-test".into(), vec![vec![b'a'; 161]]),
            Err(FieldMapError::TotalSizeTooBig)
        );
        assert_eq!(
            map.append("x-test".into(), vec![b'a'; 161]),
            Err(FieldMapError::TotalSizeTooBig)
        );
        assert_eq!(map.size, size);
        assert_eq!(map["x-test"], "kept");
        assert_eq!(
            map.set(
                "x-test".into(),
                vec![b"valid".to_vec(), b"bad\r\nvalue".to_vec()]
            ),
            Err(FieldMapError::InvalidHeaderValue)
        );
        assert_eq!(map["x-test"], "kept");
        assert_eq!(map.size, size);
        map.remove_all("X-Test".into()).unwrap();
        assert_eq!(map.size, 0);
    }

    #[test]
    fn snapshots_case_folding_value_order_and_forbidden_fields() {
        let mut map = FieldMap::new_mutable(1024);
        map.append("X-Test".into(), b"first".to_vec()).unwrap();
        map.append("x-test".into(), b"second".to_vec()).unwrap();
        let mut snapshot = map.clone();
        snapshot.set_immutable();
        map.set("X-TEST".into(), vec![b"replacement".to_vec()])
            .unwrap();
        assert_eq!(
            snapshot
                .get_all("x-test")
                .iter()
                .map(|x| x.as_bytes())
                .collect::<Vec<_>>(),
            vec![&b"first"[..], &b"second"[..]]
        );
        assert_eq!(map["x-test"], "replacement");
        for name in [
            "Host",
            "Connection",
            "Transfer-Encoding",
            "Proxy-Authorization",
            "Upgrade",
        ] {
            assert_eq!(
                map.append(name.into(), b"forbidden".to_vec()),
                Err(FieldMapError::Forbidden)
            );
        }
        let mut native = http::HeaderMap::new();
        native.insert("host", "secret".parse().unwrap());
        native.insert("x-test", "public".parse().unwrap());
        let received = FieldMap::new_immutable(native);
        assert!(!received.contains_key("host"));
        assert_eq!(received["x-test"], "public");
    }

    #[test]
    fn header_capacity_failure_is_atomic_and_content_length_is_strict() {
        let mut map = FieldMap::new_mutable(usize::MAX);
        map.append("x-test".into(), b"kept".to_vec()).unwrap();
        let size = map.size;
        assert_eq!(
            map.set("x-test".into(), vec![Vec::new(); 40_000]),
            Err(FieldMapError::TooManyFields)
        );
        assert_eq!(map["x-test"], "kept");
        assert_eq!(map.size, size);
        for value in ["+1", "-1", "1, 1", "18446744073709551616", " 1", ""] {
            assert_eq!(
                map.append("Content-Length".into(), value.as_bytes().to_vec()),
                Err(FieldMapError::InvalidHeaderValue)
            );
        }
        map.append("Content-Length".into(), b"0001".to_vec())
            .unwrap();
        assert_eq!(map["content-length"], "1");
    }
}
