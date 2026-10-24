//! URL parsing + relative resolution + URL-object construction.

use super::*;

use super::search_params::{create_url_search_params_object, parse_query_string};

/// WHATWG "special scheme": has a host, and its parser treats `\` as a path
/// separator (`foo:`, `mailto:`, `data:` paths keep it).
pub(crate) fn is_special_url_scheme(protocol: &str) -> bool {
    is_special_scheme_name(protocol.strip_suffix(':').unwrap_or(protocol))
}

fn is_special_scheme_name(scheme: &str) -> bool {
    ["ftp", "file", "http", "https", "ws", "wss"]
        .iter()
        .any(|name| scheme.eq_ignore_ascii_case(name))
}

fn special_scheme_prefix(input: &str) -> bool {
    input
        .find(':')
        .is_some_and(|colon| is_special_scheme_name(&input[..colon]))
}

/// Simple URL parser
/// Returns (protocol, host, hostname, port, pathname, search, hash)
pub(crate) fn parse_url(url_str: &str) -> (String, String, String, String, String, String, String) {
    let normalized_url = normalize_file_drive_authority(url_str);
    let url_str = normalized_url.as_str();
    let mut protocol = String::new();
    // #854: `host`, `hostname`, `pathname` are always set by the file: /
    // non-file branches below, so the initial empty strings were dead
    // writes. Declared without an initial value — Rust will catch any
    // future code path that fails to assign before use.
    let mut host: String;
    let hostname: String;
    let pathname: String;
    let mut port = String::new();
    let mut search = String::new();
    let mut hash = String::new();

    let mut remaining = url_str;

    // Extract hash (fragment)
    if let Some(hash_idx) = remaining.find('#') {
        hash = remaining[hash_idx..].to_string();
        remaining = &remaining[..hash_idx];
    }

    // Extract search (query string)
    if let Some(query_idx) = remaining.find('?') {
        search = remaining[query_idx..].to_string();
        remaining = &remaining[..query_idx];
    }

    // A special scheme's `\` is a separator, including where the authority
    // marker sits. Query and fragment keep theirs (their encode sets don't
    // encode it) and were split off above, so the fold happens here.
    let folded;
    let mut remaining = if special_scheme_prefix(remaining) && remaining.contains('\\') {
        folded = remaining.replace('\\', "/");
        folded.as_str()
    } else {
        remaining
    };

    // Track whether the `file:` URL carried a `//` authority marker so the
    // file branch below can split out a UNC-style host (`file://host/path`)
    // from a hostless path (`file:///path`, `file:/path`). #2975
    let mut file_had_authority = false;

    // Extract protocol. WHATWG canonicalization lowercases the scheme, so
    // `HTTP://...` parses identically to `http://...` (and default-port
    // stripping / special-scheme detection below see the lowered form). #2974
    if let Some(proto_idx) = remaining.find("://") {
        protocol = format!("{}:", remaining[..proto_idx].to_ascii_lowercase());
        file_had_authority = protocol == "file:";
        remaining = &remaining[proto_idx + 3..];
    } else if remaining.starts_with("file:") {
        protocol = "file:".to_string();
        remaining = remaining.strip_prefix("file:").unwrap_or(remaining);
        // Handle file:/// paths
        if remaining.starts_with("//") {
            file_had_authority = true;
            remaining = remaining.strip_prefix("//").unwrap_or(remaining);
        }
    } else if let Some(colon_idx) = remaining.find(':') {
        // A special scheme is always hierarchical (the `//` is optional), so
        // only the other schemes are opaque.
        let scheme = &remaining[..colon_idx];
        let scheme_ok = !scheme.is_empty()
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
        if scheme_ok {
            protocol = format!("{}:", scheme.to_ascii_lowercase());
            if is_special_url_scheme(&protocol) {
                remaining = &remaining[colon_idx + 1..];
            } else {
                pathname = remaining[colon_idx + 1..].to_string();
                return (
                    protocol,
                    String::new(),
                    String::new(),
                    String::new(),
                    pathname,
                    search,
                    hash,
                );
            }
        }
    }

    let special = is_special_url_scheme(&protocol);
    if special && protocol != "file:" {
        // A special scheme skips the whole separator run. `file:` keeps its
        // extra separators as path (`file:////x` is the path `//x`).
        remaining = remaining.trim_start_matches('/');
    }

    // For file: URLs, the rest is the pathname — unless an authority was
    // present and its first segment is a non-empty host (`file://host/path`,
    // i.e. a UNC share). `file:///path` / `file:/path` are hostless. #2975
    if protocol == "file:" {
        if file_had_authority && !remaining.is_empty() && !remaining.starts_with('/') {
            let host_end = remaining.find('/').unwrap_or(remaining.len());
            let file_host = remaining[..host_end].to_string();
            let rest = &remaining[host_end..];
            pathname = if rest.is_empty() {
                "/".to_string()
            } else {
                rest.to_string()
            };
            // WHATWG normalizes a `localhost` file host to the empty host.
            if file_host.eq_ignore_ascii_case("localhost") {
                host = String::new();
                hostname = String::new();
            } else {
                host = file_host.clone();
                hostname = file_host;
            }
        } else {
            pathname = if remaining.is_empty() {
                "/".to_string()
            } else if remaining.starts_with('/') {
                remaining.to_string()
            } else {
                format!("/{}", remaining)
            };
            host = String::new();
            hostname = String::new();
        }
    } else {
        // Extract host and pathname. IPv6 hostnames are bracketed (`[::1]`);
        // the `:` inside brackets must not be mistaken for a port separator,
        // and the path can only start after the closing `]`.
        let path_search_start = if remaining.starts_with('[') {
            remaining
                .find(']')
                .map(|b| b + 1)
                .unwrap_or(remaining.len())
        } else {
            0
        };
        if let Some(rel_idx) = remaining[path_search_start..].find('/') {
            let path_idx = path_search_start + rel_idx;
            host = remaining[..path_idx].to_string();
            pathname = remaining[path_idx..].to_string();
        } else {
            host = remaining.to_string();
            // WHATWG: only a special scheme's path is a list that the parser
            // seeds with an empty segment, serializing as `/`. A non-special
            // scheme with an authority and no path (`foo://a:5`,
            // `mongodb://db:27017`) has an EMPTY path, so `pathname` is ""
            // and `href` carries no trailing slash (#11322).
            pathname = if matches!(
                protocol.as_str(),
                "http:" | "https:" | "ws:" | "wss:" | "ftp:"
            ) {
                "/".to_string()
            } else {
                String::new()
            };
        }

        // Extract hostname and port from host. For IPv6 the port (if any)
        // comes after the closing bracket.
        if host.starts_with('[') {
            if let Some(bracket_end) = host.find(']') {
                let after = &host[bracket_end + 1..];
                hostname = host[..=bracket_end].to_string();
                if let Some(p) = after.strip_prefix(':') {
                    if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                        port = p.to_string();
                    }
                }
            } else {
                hostname = host.clone();
            }
        } else if let Some(port_idx) = host.rfind(':') {
            let potential_port = &host[port_idx + 1..];
            if potential_port.chars().all(|c| c.is_ascii_digit()) && !potential_port.is_empty() {
                hostname = host[..port_idx].to_string();
                port = potential_port.to_string();
            } else {
                hostname = host.clone();
            }
        } else {
            hostname = host.clone();
        }

        // Strip default ports per WHATWG so `https://example.com:443/` →
        // port "" and host "example.com" (not "example.com:443").
        let default_port = match protocol.as_str() {
            "http:" | "ws:" => "80",
            "https:" | "wss:" => "443",
            "ftp:" => "21",
            _ => "",
        };
        if !default_port.is_empty() && port == default_port {
            port.clear();
            host = hostname.clone();
        }
    }

    let pathname = normalize_path(&pathname, protocol == "file:");

    (protocol, host, hostname, port, pathname, search, hash)
}

/// Apply the WHATWG path state to a hierarchical path: the `file:` drive-letter
/// canonicalization, then dot-segment removal.
pub(crate) fn normalize_path(path: &str, file_scheme: bool) -> String {
    let drive = if file_scheme {
        normalize_file_drive_letter(path)
    } else {
        None
    };
    let path = drive.as_deref().unwrap_or(path);
    // No `.` and no `%` means no dot segment: `%2e` has no literal dot.
    if !path.starts_with('/') || !path.contains(['.', '%']) {
        return path.to_string();
    }

    let parts: Vec<&str> = path[1..].split('/').collect();
    let final_index = parts.len() - 1;
    let mut segments: Vec<&str> = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        let is_final = index == final_index;
        if is_double_dot_segment(part) {
            let keeps_drive = file_scheme
                && segments.len() == 1
                && is_normalized_windows_drive_letter(segments[0]);
            if !keeps_drive {
                segments.pop();
            }
            // A trailing dot segment leaves an empty segment behind: `/a/..`
            // is `/` while `/a/b/..` is `/a/`.
            if is_final {
                segments.push("");
            }
        } else if is_single_dot_segment(part) {
            if is_final {
                segments.push("");
            }
        } else {
            segments.push(part);
        }
    }
    format!("/{}", segments.join("/"))
}

fn normalize_file_drive_letter(path: &str) -> Option<String> {
    let rest = path.strip_prefix('/')?;
    let (first, tail) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    let bytes = first.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b'|' {
        Some(format!("/{}:{}", &first[..1], tail))
    } else {
        None
    }
}

fn is_single_dot_segment(segment: &str) -> bool {
    segment == "." || segment.eq_ignore_ascii_case("%2e")
}

fn is_double_dot_segment(segment: &str) -> bool {
    segment == ".."
        || segment.eq_ignore_ascii_case(".%2e")
        || segment.eq_ignore_ascii_case("%2e.")
        || segment.eq_ignore_ascii_case("%2e%2e")
}

fn is_normalized_windows_drive_letter(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Resolve a relative URL against a base URL
pub(crate) fn resolve_url(url_str: &str, base_str: &str) -> String {
    // If url_str is already absolute, return it
    if url_str.contains("://") || url_str.starts_with("file:") {
        return url_str.to_string();
    }

    let (base_protocol, base_host, _, _, base_pathname, base_search, _) = parse_url(base_str);

    if url_str.starts_with('?') {
        if base_protocol == "file:" || base_host.is_empty() {
            return format!("{}{}{}", base_protocol, base_pathname, url_str);
        }
        return format!(
            "{}//{}{}{}",
            base_protocol, base_host, base_pathname, url_str
        );
    }

    if url_str.starts_with('#') {
        if base_protocol == "file:" || base_host.is_empty() {
            return format!(
                "{}{}{}{}",
                base_protocol, base_pathname, base_search, url_str
            );
        }
        return format!(
            "{}//{}{}{}{}",
            base_protocol, base_host, base_pathname, base_search, url_str
        );
    }

    if url_str.starts_with("//") {
        // Protocol-relative URL
        return format!("{}{}", base_protocol, url_str);
    }

    if url_str.starts_with('/') {
        // Absolute path
        if base_protocol == "file:" {
            return format!("{}{}", base_protocol, url_str);
        }
        return format!("{}//{}{}", base_protocol, base_host, url_str);
    }

    // Relative path - resolve against base pathname
    let base_dir = if base_pathname.ends_with('/') {
        base_pathname.clone()
    } else {
        // Get directory part of base pathname
        match base_pathname.rfind('/') {
            Some(idx) => base_pathname[..=idx].to_string(),
            None => "/".to_string(),
        }
    };

    // Handle . and .. in relative path
    let mut segments: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();

    for part in url_str.split('/') {
        match part {
            "." | "" => continue,
            ".." => {
                segments.pop();
            }
            _ => segments.push(part),
        }
    }

    let mut resolved_path = format!("/{}", segments.join("/"));
    // A trailing slash is semantically significant for URL resolution: the
    // result remains a directory base for a subsequent `new URL(...)` call.
    // Dropping it made `new URL("cert.pem", new URL("../fixtures/", base))`
    // resolve next to `fixtures` instead of inside it (#6765).
    if url_str.ends_with('/') && !resolved_path.ends_with('/') {
        resolved_path.push('/');
    }

    if base_protocol == "file:" {
        format!("{}{}", base_protocol, resolved_path)
    } else {
        format!("{}//{}{}", base_protocol, base_host, resolved_path)
    }
}

/// Field indices for URL object
pub(crate) const URL_HREF: u32 = 0;
pub(crate) const URL_PROTOCOL: u32 = 1;
pub(crate) const URL_HOST: u32 = 2;
pub(crate) const URL_HOSTNAME: u32 = 3;
pub(crate) const URL_PORT: u32 = 4;
pub(crate) const URL_PATHNAME: u32 = 5;
pub(crate) const URL_SEARCH: u32 = 6;
pub(crate) const URL_HASH: u32 = 7;
pub(crate) const URL_ORIGIN: u32 = 8;
pub(crate) const URL_SEARCH_PARAMS: u32 = 9;
// Issue #650: username + password fields. Pre-fix `u.username` /
// `u.password` returned undefined for `https://user:pass@example.com`
// URLs because parse_url's authority-component decomposition didn't split
// userinfo out of `host`. Post-processing in create_url_object extracts
// userinfo before the object is built.
pub(crate) const URL_USERNAME: u32 = 10;
pub(crate) const URL_PASSWORD: u32 = 11;
pub(crate) const URL_FIELD_COUNT: u32 = 12;

/// Percent-encode bytes outside the printable ASCII range (`< 0x20` and
/// `>= 0x80`). Mirrors the practical effect of the WHATWG path / query
/// percent-encode set for the common case of Unicode literals in URLs —
/// e.g. `/café/` → `/caf%C3%A9/`, `?q=á` → `?q=%C3%A1`. ASCII percent
/// sequences in the input pass through untouched.
pub(crate) fn encode_non_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if !(0x20..0x7F).contains(&b) {
            out.push_str(&format!("%{:02X}", b));
        } else {
            out.push(b as char);
        }
    }
    out
}

/// Create a URL object from a string
pub(crate) fn create_url_object(url_string: &str) -> *mut ObjectHeader {
    let (protocol, mut host, mut hostname, port, pathname, search, hash) = parse_url(url_string);
    let pathname = super::url_class::percent_encode_path(&pathname);
    let search = encode_non_ascii(&search);
    let hash = encode_non_ascii(&hash);

    // Issue #650: extract userinfo (`user:pass@`) from `host`. parse_url
    // leaves it as a prefix on host/hostname because the WHATWG authority
    // component decomposition (userinfo@host:port) wasn't implemented;
    // pre-fix `host`/`hostname`/`origin` all carried the userinfo prefix
    // and `username`/`password` came back undefined. This post-processing
    // pulls the userinfo back out before the object is built. Done here
    // (not in parse_url itself) to keep parse_url's existing 7-tuple
    // signature stable for resolve_url and the unit tests — only the
    // create_url_object path needs the split.
    let mut username = String::new();
    let mut password = String::new();
    if let Some(at_idx) = host.rfind('@') {
        let userinfo = host[..at_idx].to_string();
        if let Some(colon_idx) = userinfo.find(':') {
            username = userinfo[..colon_idx].to_string();
            password = userinfo[colon_idx + 1..].to_string();
        } else {
            username = userinfo;
        }
        let rest = host[at_idx + 1..].to_string();
        host = rest.clone();
        // Re-derive hostname from the cleaned host (drop port if present).
        hostname = if let Some(port_idx) = rest.rfind(':') {
            let p = &rest[port_idx + 1..];
            if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() {
                rest[..port_idx].to_string()
            } else {
                rest
            }
        } else {
            rest
        };
    }

    // WHATWG host canonicalization (#2974): special schemes lowercase the
    // host and IDNA/punycode-encode Unicode labels (`bücher.example` →
    // `xn--bcher-kva.example`, `EXAMPLE.COM` → `example.com`). Numeric /
    // IPv4-shorthand hosts also canonicalize to dotted-quad. We reuse the
    // `url`-crate-backed `whatwg_canonicalize_host` (reparses `http://<host>/`),
    // which is the same path the hostname setter uses. Only special hierarchical
    // schemes get host canonicalization; opaque schemes have no host, and
    // `file:` is one of them for this purpose too (`file://SERVER/x` → `server`).
    let is_special = is_special_url_scheme(&protocol);
    if is_special && !hostname.is_empty() {
        if let Some(canon) = super::whatwg_canonicalize_host(&hostname) {
            hostname = canon.clone();
            host = if port.is_empty() {
                canon
            } else {
                format!("{}:{}", canon, port)
            };
        }
    }

    // Construct the full href (now includes userinfo when present, per
    // WHATWG: `scheme://[user[:pass]@]host[:port][/path][?query][#frag]`).
    let userinfo_prefix = if !username.is_empty() || !password.is_empty() {
        if password.is_empty() {
            format!("{}@", username)
        } else {
            format!("{}:{}@", username, password)
        }
    } else {
        String::new()
    };
    let href = if protocol == "file:" {
        format!("{}//{}{}{}{}", protocol, host, pathname, search, hash)
    } else if host.is_empty() {
        // Opaque schemes (`mailto:`, `data:`, `urn:`) — keep the scheme prefix.
        format!("{}{}{}{}", protocol, pathname, search, hash)
    } else {
        format!(
            "{}//{}{}{}{}{}",
            protocol, userinfo_prefix, host, pathname, search, hash
        )
    };

    // Calculate origin (intentionally excludes userinfo per WHATWG spec —
    // origin is the "registered domain" identity, not the credentials).
    let origin = if protocol == "file:" {
        "null".to_string() // file: URLs have "null" origin
    } else if host.is_empty() {
        "null".to_string()
    } else {
        format!("{}//{}", protocol, host)
    };

    // Component storage and its intrinsic prototype are declared at startup.
    let obj = unsafe {
        let obj = crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(
            crate::object::runtime_layouts::URL_COMPONENTS
        ));
        // GC_STORE_AUDIT(POINTER_FREE): immutable runtime family brand.
        (*obj).class_id = crate::object::runtime_layouts::URL_CLASS_ID;
        obj
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));

    // The declared private slots hold URL state. Public names remain on
    // URL.prototype; expandos append after the private slots without seeding
    // a tombstone layout on the first write.

    // Set all the URL properties
    let set_string = |field, text: &str| {
        let value = create_string_f64(text);
        let obj = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader;
        js_object_set_field_f64(obj, field, value);
    };
    set_string(URL_HREF, &href);
    set_string(URL_PROTOCOL, &protocol);
    set_string(URL_HOST, &host);
    set_string(URL_HOSTNAME, &hostname);
    set_string(URL_PORT, &port);
    set_string(URL_PATHNAME, &pathname);
    set_string(URL_SEARCH, &search);
    set_string(URL_HASH, &hash);
    set_string(URL_ORIGIN, &origin);
    set_string(URL_USERNAME, &username);
    set_string(URL_PASSWORD, &password);
    // Build a real URLSearchParams object from the search string (parsed
    // lazily below). Storing a string here would break `url.searchParams.get()`
    // because the URLSearchParams method runtime functions interpret the
    // receiver as `*mut ObjectHeader` and would deref a StringHeader
    // instead — see issue #111.
    let params_entries = parse_query_string(&search);
    let params_obj = create_url_search_params_object(params_entries);
    let params_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(params_obj as i64));
    let obj = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field_f64(obj, URL_SEARCH_PARAMS, params_h.get_nanbox_f64());
    // Adopt: subsequent mutations on `url.searchParams` should sync back.
    let params_obj =
        crate::value::js_nanbox_get_pointer(params_h.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field_f64(
        params_obj,
        super::search_params::URL_SEARCH_PARAMS_OWNER,
        obj_h.get_nanbox_f64(),
    );

    // %URL.prototype%, not `globalThis.URL.prototype`: a module-level
    // `function URL` shadowing the global made every native URL inherit the
    // user function's prototype, so `.hostname` & co. read undefined (#11585).
    // The global read only materializes the builtin the first time.
    let intrinsic =
        || crate::object::URL_INTRINSIC_PROTO_PTR.load(std::sync::atomic::Ordering::Acquire);
    let proto = if intrinsic() != 0 {
        crate::value::js_nanbox_pointer(intrinsic())
    } else {
        // Materializes the builtin (which records the intrinsic) on first use.
        let global = crate::object::builtin_prototype_value("URL");
        match intrinsic() {
            0 => global,
            ptr => crate::value::js_nanbox_pointer(ptr),
        }
    };
    debug_assert_ne!(proto.to_bits(), crate::value::TAG_UNDEFINED);
    crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader
}

/// Build and throw a `TypeError` matching Node's WHATWG-URL parser's
/// "Invalid URL" exception. Used by `new URL(...)` when the input doesn't
/// look like a parseable absolute URL.
pub(crate) fn throw_invalid_url(input: &str) -> ! {
    let msg = format!("Invalid URL: {}", input);
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_INVALID_URL");
    let err = crate::error::js_typeerror_new(msg_ptr);
    let err_val = crate::value::js_nanbox_pointer(err as i64);
    crate::exception::js_throw(err_val);
}

/// Issue #650: re-derive the URL's `href` field from the current
/// component fields after a setter has mutated one of them. Mirrors the
/// composition logic in `create_url_object`. Caller must hold a non-null
/// URL object.
pub(crate) unsafe fn rebuild_url_href(url: *mut ObjectHeader) {
    let protocol = get_string_content(crate::object::js_object_get_field_f64(url, URL_PROTOCOL));
    let host = get_string_content(crate::object::js_object_get_field_f64(url, URL_HOST));
    let pathname = get_string_content(crate::object::js_object_get_field_f64(url, URL_PATHNAME));
    let search = get_string_content(crate::object::js_object_get_field_f64(url, URL_SEARCH));
    let hash = get_string_content(crate::object::js_object_get_field_f64(url, URL_HASH));
    let username = get_string_content(crate::object::js_object_get_field_f64(url, URL_USERNAME));
    let password = get_string_content(crate::object::js_object_get_field_f64(url, URL_PASSWORD));

    let userinfo_prefix = if !username.is_empty() || !password.is_empty() {
        if password.is_empty() {
            format!("{}@", username)
        } else {
            format!("{}:{}@", username, password)
        }
    } else {
        String::new()
    };
    let href = if protocol == "file:" {
        format!("{}//{}{}{}{}", protocol, host, pathname, search, hash)
    } else if host.is_empty() {
        // Opaque schemes (`mailto:`, `data:`, `urn:`) — keep the scheme prefix.
        format!("{}{}{}{}", protocol, pathname, search, hash)
    } else {
        format!(
            "{}//{}{}{}{}{}",
            protocol, userinfo_prefix, host, pathname, search, hash
        )
    };
    js_object_set_field_f64(url, URL_HREF, create_string_f64(&href));
}

pub(crate) unsafe fn rebuild_url_origin(url: *mut ObjectHeader) {
    let protocol = get_string_content(crate::object::js_object_get_field_f64(url, URL_PROTOCOL));
    let host = get_string_content(crate::object::js_object_get_field_f64(url, URL_HOST));
    let origin = if protocol == "file:" || host.is_empty() {
        "null".to_string()
    } else {
        format!("{}//{}", protocol, host)
    };
    js_object_set_field_f64(url, URL_ORIGIN, create_string_f64(&origin));
}

/// Recompose `host` (`hostname[:port]`) from the URL's `hostname` and `port`
/// fields, stripping default ports for known hierarchical schemes.
pub(crate) unsafe fn rebuild_url_host(url: *mut ObjectHeader) {
    let protocol = get_string_content(crate::object::js_object_get_field_f64(url, URL_PROTOCOL));
    let hostname = get_string_content(crate::object::js_object_get_field_f64(url, URL_HOSTNAME));
    let mut port = get_string_content(crate::object::js_object_get_field_f64(url, URL_PORT));
    let default_port = match protocol.as_str() {
        "http:" | "ws:" => "80",
        "https:" | "wss:" => "443",
        "ftp:" => "21",
        _ => "",
    };
    if !default_port.is_empty() && port == default_port {
        port.clear();
        js_object_set_field_f64(url, URL_PORT, create_string_f64(""));
    }
    let host = if port.is_empty() {
        hostname
    } else {
        format!("{}:{}", hostname, port)
    };
    js_object_set_field_f64(url, URL_HOST, create_string_f64(&host));
    rebuild_url_origin(url);
}

/// Validate that `s` looks like a parseable absolute URL — has a scheme
/// followed by `:` and either `//` (for hierarchical schemes) or non-empty
/// scheme-specific data. Used by `URL.canParse` / `URL.parse` to mirror
/// the validation Node's WHATWG parser performs before constructing.
pub(crate) fn is_valid_absolute_url(s: &str) -> bool {
    let normalized = normalize_file_drive_authority(s.trim());
    let s = normalized.as_str();
    if s.is_empty() {
        return false;
    }
    // Scheme: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
    let mut chars = s.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let mut scheme_end = 1;
    for c in chars {
        if c == ':' {
            break;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
        scheme_end += 1;
        if scheme_end > 64 {
            // Practical cap; real schemes are short.
            return false;
        }
    }
    if scheme_end >= s.len() || s.as_bytes()[scheme_end] != b':' {
        return false;
    }
    let after_scheme = &s[scheme_end + 1..];
    let scheme = &s[..scheme_end];
    if is_special_scheme_name(scheme) {
        let folded;
        let after_scheme = if after_scheme.contains('\\') {
            folded = after_scheme.replace('\\', "/");
            folded.as_str()
        } else {
            after_scheme
        };
        // The `//` is optional for a special scheme, and `file:` also allows a
        // hostless path.
        if scheme == "file" {
            if let Some(authority) = after_scheme.strip_prefix("//") {
                let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
                if !is_valid_url_authority(scheme, authority) {
                    return false;
                }
            }
        } else {
            let rest = after_scheme.trim_start_matches('/');
            let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
            if !is_valid_url_authority(scheme, authority) {
                return false;
            }
        }
    } else if let Some(authority) = after_scheme.strip_prefix("//") {
        // `\` is a forbidden host code point, so `foo://a\b` is not a URL.
        let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        if host.contains('\\') {
            return false;
        }
    }
    true
}

/// WHATWG treats a Windows drive letter after `file://` as the beginning of
/// the path, not as an authority host. Backslashes are path separators in
/// this file-URL form, but query and fragment bytes are left untouched.
pub(super) fn normalize_file_drive_authority(input: &str) -> String {
    let Some(colon) = input.find(':') else {
        return input.to_string();
    };
    if !input[..colon].eq_ignore_ascii_case("file") || !input[colon + 1..].starts_with("//") {
        return input.to_string();
    }

    let authority_start = colon + 3;
    let url_end = input[authority_start..]
        .find(['?', '#'])
        .map_or(input.len(), |offset| authority_start + offset);
    let tail = &input[url_end..];
    let authority_and_path = &input[authority_start..url_end];
    let drive_end = authority_and_path
        .find(['/', '\\'])
        .unwrap_or(authority_and_path.len());
    let authority = &authority_and_path[..drive_end];
    let drive_bytes = authority.as_bytes();
    if drive_bytes.len() != 2
        || !drive_bytes[0].is_ascii_alphabetic()
        || !matches!(drive_bytes[1], b':' | b'|')
    {
        return input.to_string();
    }

    let rest = authority_and_path[drive_end..].replace('\\', "/");
    format!("{}///{}{}{}", &input[..=colon], authority, rest, tail)
}

fn is_valid_url_authority(scheme: &str, authority: &str) -> bool {
    // file:// is allowed to have an empty host, every other special scheme
    // needs one.
    if scheme != "file" && authority.is_empty() {
        return false;
    }
    if authority.bytes().any(|b| b <= 0x20 || b == 0x7f) {
        return false;
    }

    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if scheme != "file" && host_port.is_empty() {
        return false;
    }

    let host = if let Some(rest) = host_port.strip_prefix('[') {
        let Some(bracket_end) = rest.find(']') else {
            return false;
        };
        let after_bracket = &rest[bracket_end + 1..];
        if !after_bracket.is_empty()
            && !after_bracket
                .strip_prefix(':')
                .is_some_and(|port| port.bytes().all(|b| b.is_ascii_digit()))
        {
            return false;
        }
        &host_port[..=bracket_end + 1]
    } else if let Some(port_idx) = host_port.rfind(':') {
        let port = &host_port[port_idx + 1..];
        if !port.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        &host_port[..port_idx]
    } else {
        host_port
    };

    if scheme != "file" && host.is_empty() {
        return false;
    }
    !has_invalid_percent_escape(host)
}

fn has_invalid_percent_escape(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                return true;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    false
}
