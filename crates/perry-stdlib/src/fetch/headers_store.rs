//! `HeadersStore` — the insertion-ordered (lowercase-name, value) list behind
//! every `Headers` handle.
//!
//! Split out of `fetch/mod.rs` to keep that file under the 2000-line cap
//! (`scripts/check_file_size.sh`). Nothing about the types changed; the
//! visibility is widened from private-to-`mod.rs` to `pub(super)` because the
//! items now live one module down.

#[derive(Clone, Default)]
pub(super) struct HeadersStore {
    /// (lowercase_name, value) entries — insertion order preserved
    pub(super) entries: Vec<(String, String)>,
}

impl HeadersStore {
    pub(super) fn set(&mut self, key: &str, value: &str) {
        let lk = key.to_ascii_lowercase();
        self.entries.retain(|(k, _)| *k != lk);
        self.entries.push((lk, value.to_string()));
    }
    /// Web Fetch `Headers.append` — combines repeated normal headers with
    /// `", "`, but keeps `Set-Cookie` values as separate entries so
    /// `getSetCookie()` can return them individually.
    pub(super) fn append(&mut self, key: &str, value: &str) {
        let lk = key.to_ascii_lowercase();
        if lk == "set-cookie" {
            self.entries.push((lk, value.to_string()));
            return;
        }
        for entry in self.entries.iter_mut() {
            if entry.0 == lk {
                entry.1.push_str(", ");
                entry.1.push_str(value);
                return;
            }
        }
        self.entries.push((lk, value.to_string()));
    }
    pub(super) fn get(&self, key: &str) -> Option<String> {
        let lk = key.to_ascii_lowercase();
        if lk == "set-cookie" {
            let values: Vec<&str> = self
                .entries
                .iter()
                .filter(|(k, _)| *k == lk)
                .map(|(_, v)| v.as_str())
                .collect();
            if values.is_empty() {
                None
            } else {
                Some(values.join(", "))
            }
        } else {
            self.entries
                .iter()
                .find(|(k, _)| *k == lk)
                .map(|(_, v)| v.clone())
        }
    }
    pub(super) fn has(&self, key: &str) -> bool {
        let lk = key.to_ascii_lowercase();
        self.entries.iter().any(|(k, _)| *k == lk)
    }
    pub(super) fn delete(&mut self, key: &str) {
        let lk = key.to_ascii_lowercase();
        self.entries.retain(|(k, _)| *k != lk);
    }
    pub(super) fn set_cookie_values(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(k, _)| k == "set-cookie")
            .map(|(_, v)| v.clone())
            .collect()
    }
}
