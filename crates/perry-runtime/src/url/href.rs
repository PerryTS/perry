//! A URL object's state: its serialized `href` and the component boundaries
//! inside it.
//!
//! One parse (the WHATWG parser of the `url` crate) yields the serialization
//! and the offsets where each component starts, which is the `url` crate's own
//! representation (`url::quirks::internal_components`). The object keeps
//! exactly that: the href string in its href slot and each boundary in the
//! slot of the component it starts. The getters slice href when read, and a
//! setter re-parses href, applies the WHATWG setter, and stores both back.

use super::parse::{
    URL_HASH, URL_HOST, URL_HOSTNAME, URL_HREF, URL_ORIGIN, URL_PASSWORD, URL_PATHNAME, URL_PORT,
    URL_PROTOCOL, URL_SEARCH, URL_SEARCH_PARAMS, URL_USERNAME,
};
use super::*;

/// Slot holding the scheme's end (the index of its `:`).
const SCHEME_END: u32 = URL_PROTOCOL;
/// Slot holding the end of the username (where `:password@` or `@` starts).
const USERNAME_END: u32 = URL_USERNAME;
/// Slot holding the start of the host.
const HOST_START: u32 = URL_HOST;
/// Slot holding the end of the host (where `:port` or the path starts).
const HOST_END: u32 = URL_HOSTNAME;
/// Slot holding the start of the path.
const PATH_START: u32 = URL_PATHNAME;
/// Slot holding the index of the query's `?`, or -1 when there is no query.
const QUERY_START: u32 = URL_SEARCH;
/// Slot holding the index of the fragment's `#`, or -1 when there is none.
const FRAGMENT_START: u32 = URL_HASH;

/// Component boundaries inside an href, as the `url` crate records them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Offsets {
    pub scheme_end: u32,
    pub username_end: u32,
    pub host_start: u32,
    pub host_end: u32,
    pub path_start: u32,
    pub query_start: Option<u32>,
    pub fragment_start: Option<u32>,
}

/// A parsed URL: the serialization and its boundaries.
pub(crate) struct Parsed {
    pub href: String,
    pub offsets: Offsets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Component {
    Href,
    Protocol,
    Username,
    Password,
    Host,
    Hostname,
    Port,
    Pathname,
    Search,
    Hash,
}

impl Offsets {
    fn has_authority(&self, href: &str) -> bool {
        href.as_bytes()
            .get(self.scheme_end as usize..)
            .is_some_and(|rest| rest.starts_with(b"://"))
    }

    /// End of the path: the query's `?`, else the fragment's `#`, else the end.
    fn path_end(&self, len: usize) -> usize {
        self.query_start
            .or(self.fragment_start)
            .map_or(len, |i| i as usize)
    }

    /// The byte range of `component` inside `href`, per `url::quirks`.
    pub(crate) fn range(&self, href: &str, component: Component) -> (usize, usize) {
        let len = href.len();
        let bytes = href.as_bytes();
        let scheme_end = self.scheme_end as usize;
        let username_end = self.username_end as usize;
        let host_start = self.host_start as usize;
        let host_end = self.host_end as usize;
        let path_start = self.path_start as usize;
        let empty = (0, 0);
        match component {
            Component::Href => (0, len),
            Component::Protocol => (0, scheme_end + 1),
            Component::Username => {
                if self.has_authority(href) {
                    (scheme_end + 3, username_end)
                } else {
                    empty
                }
            }
            Component::Password => {
                if self.has_authority(href)
                    && username_end < len
                    && bytes[username_end] == b':'
                    && host_start > username_end + 1
                {
                    (username_end + 1, host_start - 1)
                } else {
                    empty
                }
            }
            Component::Host => (host_start, path_start),
            Component::Hostname => (host_start, host_end),
            Component::Port => {
                if host_end < path_start && bytes[host_end] == b':' {
                    (host_end + 1, path_start)
                } else {
                    empty
                }
            }
            Component::Pathname => (path_start, self.path_end(len)),
            Component::Search => match self.query_start {
                Some(q) => {
                    let end = self.fragment_start.map_or(len, |f| f as usize);
                    // A lone `?` reads as "".
                    if end - q as usize <= 1 {
                        empty
                    } else {
                        (q as usize, end)
                    }
                }
                None => empty,
            },
            Component::Hash => match self.fragment_start {
                Some(f) if len - f as usize > 1 => (f as usize, len),
                _ => empty,
            },
        }
    }

    /// The raw query (after `?`, before `#`), or `None` without a query.
    pub(crate) fn query<'a>(&self, href: &'a str) -> Option<&'a str> {
        let q = self.query_start? as usize;
        let end = self.fragment_start.map_or(href.len(), |f| f as usize);
        Some(&href[q + 1..end])
    }
}

/// The URL's origin serialization (`URL.prototype.origin`).
pub(crate) fn origin(href: &str, o: &Offsets) -> String {
    let scheme = &href[..o.scheme_end as usize];
    match scheme {
        "http" | "https" | "ws" | "wss" | "ftp" => {
            let (start, end) = o.range(href, Component::Host);
            format!("{scheme}://{}", &href[start..end])
        }
        // `blob:` takes the origin of the URL its path holds, when that is an
        // http(s) URL.
        "blob" => {
            let (start, end) = o.range(href, Component::Pathname);
            match parse(&href[start..end], None) {
                Some(inner)
                    if matches!(
                        &inner.href[..inner.offsets.scheme_end as usize],
                        "http" | "https"
                    ) =>
                {
                    origin(&inner.href, &inner.offsets)
                }
                _ => "null".to_string(),
            }
        }
        _ => "null".to_string(),
    }
}

// ---------------------------------------------------------------------------
// The parser. `url-engine` builds link the `url` crate; a build without it
// has no URL object (the compiler turns the feature on wherever a URL can
// exist), so every parse there fails.
// ---------------------------------------------------------------------------

/// A WHATWG setter, applied to a parsed URL.
#[derive(Clone, Copy)]
pub(crate) enum Setter<'a> {
    Href(&'a str),
    Protocol(&'a str),
    Username(&'a str),
    Password(&'a str),
    Host(&'a str),
    Hostname(&'a str),
    Port(&'a str),
    Pathname(&'a str),
    Search(&'a str),
    Hash(&'a str),
    /// The URLSearchParams update steps: an already-serialized query, or
    /// `None` for an empty list.
    Query(Option<&'a str>),
}

#[cfg(feature = "url-engine")]
fn parsed_of(url: url::Url) -> Parsed {
    let c = url::quirks::internal_components(&url);
    Parsed {
        offsets: Offsets {
            scheme_end: c.scheme_end,
            username_end: c.username_end,
            host_start: c.host_start,
            host_end: c.host_end,
            path_start: c.path_start,
            query_start: c.query_start,
            fragment_start: c.fragment_start,
        },
        href: url.into(),
    }
}

/// Parse `input`, against `base` when given. `None` when either fails.
#[cfg(feature = "url-engine")]
pub(crate) fn parse(input: &str, base: Option<&str>) -> Option<Parsed> {
    let url = match base {
        None => url::Url::parse(input).ok()?,
        Some(base) => {
            let base = url::Url::parse(base).ok()?;
            url::Url::options().base_url(Some(&base)).parse(input).ok()?
        }
    };
    Some(parsed_of(url))
}

#[cfg(not(feature = "url-engine"))]
pub(crate) fn parse(_input: &str, _base: Option<&str>) -> Option<Parsed> {
    None
}

/// Apply `setter` to the URL serialized as `href`. `Err` only for an `href`
/// that does not parse (the `href` setter throws); a setter whose value the
/// WHATWG steps reject leaves the URL as it was (`Ok` with the same href).
#[cfg(feature = "url-engine")]
pub(crate) fn apply(href: &str, setter: Setter<'_>) -> Result<Parsed, ()> {
    use url::quirks;
    let Ok(mut url) = url::Url::parse(href) else {
        return Err(());
    };
    match setter {
        Setter::Href(v) => quirks::set_href(&mut url, v).map_err(|_| ())?,
        Setter::Protocol(v) => {
            let _ = quirks::set_protocol(&mut url, v);
        }
        Setter::Username(v) => {
            let _ = quirks::set_username(&mut url, v);
        }
        Setter::Password(v) => {
            let _ = quirks::set_password(&mut url, v);
        }
        Setter::Host(v) => {
            let _ = quirks::set_host(&mut url, v);
        }
        Setter::Hostname(v) => {
            let _ = quirks::set_hostname(&mut url, v);
        }
        Setter::Port(v) => {
            let _ = quirks::set_port(&mut url, v);
        }
        Setter::Pathname(v) => quirks::set_pathname(&mut url, v),
        Setter::Search(v) => quirks::set_search(&mut url, v),
        Setter::Hash(v) => quirks::set_hash(&mut url, v),
        Setter::Query(v) => url.set_query(v),
    }
    Ok(parsed_of(url))
}

#[cfg(not(feature = "url-engine"))]
pub(crate) fn apply(_href: &str, _setter: Setter<'_>) -> Result<Parsed, ()> {
    Err(())
}

// ---------------------------------------------------------------------------
// The object.
// ---------------------------------------------------------------------------

fn offset_value(offset: Option<u32>) -> f64 {
    offset.map_or(-1.0, f64::from)
}

/// Write `parsed` into the URL held by `url_h`: a new href string and its
/// boundaries. The searchParams slot is left alone.
fn store(url_h: &crate::gc::RuntimeHandle<'_>, parsed: &Parsed) {
    let href = create_string_f64(&parsed.href);
    let url = crate::value::js_nanbox_get_pointer(url_h.get_nanbox_f64()) as *mut ObjectHeader;
    let o = &parsed.offsets;
    js_object_set_field_f64(url, URL_HREF, href);
    js_object_set_field_f64(url, SCHEME_END, f64::from(o.scheme_end));
    js_object_set_field_f64(url, USERNAME_END, f64::from(o.username_end));
    js_object_set_field_f64(url, HOST_START, f64::from(o.host_start));
    js_object_set_field_f64(url, HOST_END, f64::from(o.host_end));
    js_object_set_field_f64(url, PATH_START, f64::from(o.path_start));
    js_object_set_field_f64(url, QUERY_START, offset_value(o.query_start));
    js_object_set_field_f64(url, FRAGMENT_START, offset_value(o.fragment_start));
}

/// A new URL object (layout `URL_COMPONENTS`, born final) holding `parsed`.
pub(crate) fn new_url_object(parsed: &Parsed) -> *mut ObjectHeader {
    // The layout's prototype family names %URL.prototype%; the first URL
    // materializes the builtin so the instance's [[Prototype]] word is set.
    if crate::object::URL_INTRINSIC_PROTO_PTR.load(std::sync::atomic::Ordering::Acquire) == 0 {
        let _ = crate::object::builtin_prototype_value("URL");
    }
    let obj = unsafe {
        let obj = crate::object::layout_desc::perry_alloc_layout(
            crate::object::runtime_layouts::URL_COMPONENTS::desc(),
        );
        // GC_STORE_AUDIT(POINTER_FREE): immutable runtime family brand.
        (*obj).class_id = crate::object::runtime_layouts::URL_CLASS_ID;
        obj
    };
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    for slot in [URL_PORT, URL_ORIGIN, URL_SEARCH_PARAMS, URL_PASSWORD] {
        js_object_set_field_f64(obj, slot, undefined);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    store(&obj_h, parsed);
    crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader
}

/// The href string of a URL object (its href slot).
pub(crate) fn href_value(url: *mut ObjectHeader) -> f64 {
    crate::object::js_object_get_field_f64(url, URL_HREF)
}

fn slot_u32(url: *mut ObjectHeader, slot: u32) -> u32 {
    crate::object::js_object_get_field_f64(url, slot) as u32
}

fn slot_offset(url: *mut ObjectHeader, slot: u32) -> Option<u32> {
    let v = crate::object::js_object_get_field_f64(url, slot);
    (v >= 0.0).then_some(v as u32)
}

/// The URL's boundaries.
pub(crate) fn offsets(url: *mut ObjectHeader) -> Offsets {
    Offsets {
        scheme_end: slot_u32(url, SCHEME_END),
        username_end: slot_u32(url, USERNAME_END),
        host_start: slot_u32(url, HOST_START),
        host_end: slot_u32(url, HOST_END),
        path_start: slot_u32(url, PATH_START),
        query_start: slot_offset(url, QUERY_START),
        fragment_start: slot_offset(url, FRAGMENT_START),
    }
}

/// The href string header and its text. A URL's serialization is ASCII.
///
/// # Safety
/// `url` is a live URL object. The returned text borrows the href string,
/// so it is valid only until the next allocation.
unsafe fn href_text<'a>(url: *mut ObjectHeader) -> (*const crate::StringHeader, &'a str) {
    let v = href_value(url);
    let s = crate::value::js_nanbox_get_pointer(v) as *const crate::StringHeader;
    let bytes = std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
    (s, std::str::from_utf8_unchecked(bytes))
}

/// The href as an owned Rust string.
pub(crate) fn href_string(url: *mut ObjectHeader) -> String {
    unsafe { href_text(url).1.to_string() }
}

/// `url.<component>` as a JS string: a slice of href.
pub(crate) fn component_value(url: *mut ObjectHeader, component: Component) -> f64 {
    let (s, href) = unsafe { href_text(url) };
    let (start, end) = offsets(url).range(href, component);
    if start == 0 && end == href.len() {
        return href_value(url);
    }
    let len = (end - start) as u32;
    let part = crate::string::string_copy_range(
        s,
        start,
        len,
        len,
        crate::string::STRING_FLAG_WTF8_VALIDATED,
    );
    crate::value::js_nanbox_string(part as i64)
}

/// `url.<component>` as a Rust string.
pub(crate) fn component_string(url: *mut ObjectHeader, component: Component) -> String {
    let (_, href) = unsafe { href_text(url) };
    let (start, end) = offsets(url).range(href, component);
    href[start..end].to_string()
}

/// `url.origin` as a JS string.
pub(crate) fn origin_value(url: *mut ObjectHeader) -> f64 {
    let (_, href) = unsafe { href_text(url) };
    let text = origin(href, &offsets(url));
    create_string_f64(&text)
}

/// The URL's raw query (`None` without one), owned.
pub(crate) fn query_string(url: *mut ObjectHeader) -> Option<String> {
    let (_, href) = unsafe { href_text(url) };
    offsets(url).query(href).map(str::to_string)
}

/// `url.<component>` of an object that may not be a URL: "" for any other
/// object. For the fs / process paths that accept "a URL or something else".
pub(crate) fn component_of(obj: *mut ObjectHeader, component: Component) -> String {
    if crate::object::url_layout::is_url_object_shape(obj) {
        component_string(obj, component)
    } else {
        String::new()
    }
}

/// Run `setter` on `url` and store the result. `Err` when the href setter's
/// value does not parse. The `href` and `search` setters also reset the list
/// of the URL's searchParams object, when it has one.
pub(crate) fn set(url: *mut ObjectHeader, setter: Setter<'_>) -> Result<(), ()> {
    let parsed = apply(&href_string(url), setter)?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let url_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(url as i64));
    store(&url_h, &parsed);
    if matches!(setter, Setter::Href(_) | Setter::Search(_)) {
        let url = crate::value::js_nanbox_get_pointer(url_h.get_nanbox_f64()) as *mut ObjectHeader;
        super::search_params::reset_url_search_params(url);
    }
    Ok(())
}

#[cfg(all(test, feature = "url-engine"))]
mod tests {
    use super::*;

    fn parts(input: &str, base: Option<&str>) -> Vec<String> {
        let p = parse(input, base).expect("parses");
        let mut out = vec![p.href.clone()];
        for c in [
            Component::Protocol,
            Component::Username,
            Component::Password,
            Component::Host,
            Component::Hostname,
            Component::Port,
            Component::Pathname,
            Component::Search,
            Component::Hash,
        ] {
            let (s, e) = p.offsets.range(&p.href, c);
            out.push(p.href[s..e].to_string());
        }
        out.push(origin(&p.href, &p.offsets));
        out
    }

    #[test]
    fn components_slice_href() {
        assert_eq!(
            parts("HTTPS://u:p@EXAMPLE.com:8443/a/../b?x=1#f", None),
            [
                "https://u:p@example.com:8443/b?x=1#f",
                "https:",
                "u",
                "p",
                "example.com:8443",
                "example.com",
                "8443",
                "/b",
                "?x=1",
                "#f",
                "https://example.com:8443"
            ]
        );
    }

    #[test]
    fn lone_query_and_fragment_markers_read_empty() {
        let p = parts("http://h/p?#", None);
        assert_eq!(p[0], "http://h/p?#");
        assert_eq!((p[8].as_str(), p[9].as_str()), ("", ""));
    }

    #[test]
    fn relative_parses_against_the_base_once() {
        assert_eq!(parts("/api/hello?name=perry", Some("http://n"))[0], "http://n/api/hello?name=perry");
        assert!(parse("/x", Some("not a url")).is_none());
        assert!(parse("x", None).is_none());
    }

    #[test]
    fn opaque_file_ipv6_idna() {
        assert_eq!(parts("mailto:a@b", None)[7], "a@b");
        assert_eq!(parts("mailto:a@b", None)[10], "null");
        assert_eq!(parts("file:///C:/x/../y", None)[0], "file:///C:/y");
        assert_eq!(parts("http://[::1]:80/", None)[4], "[::1]");
        assert_eq!(parts("http://bücher.example/", None)[5], "xn--bcher-kva.example");
        assert_eq!(parts("blob:https://a.b/x", None)[10], "https://a.b");
    }

    #[test]
    fn setters_round_trip() {
        let href = "http://h/p?a=1#f";
        assert_eq!(apply(href, Setter::Search("b=2")).unwrap().href, "http://h/p?b=2#f");
        assert_eq!(apply(href, Setter::Query(None)).unwrap().href, "http://h/p#f");
        assert_eq!(apply(href, Setter::Port("8080")).unwrap().href, "http://h:8080/p?a=1#f");
        assert!(apply(href, Setter::Href("nope")).is_err());
    }
}
