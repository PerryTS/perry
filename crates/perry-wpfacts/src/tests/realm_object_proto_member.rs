//! Skin seed 1 (three.js FileLoader::load, `new Request(url, {...})`): a
//! member that the method lookup on a realm-declared object's chain finds
//! on `Object.prototype` is `Object.prototype`'s own method and runs its
//! model (`proto_method`), as on a plain object; it is no unknown code.
//! `Object.prototype.toString` on a Request reads `@@toStringTag` (a data
//! string on `Request.prototype`) and returns "[object Request]"; nothing
//! escapes. Each R-test is red when the lookup runs the member as unknown
//! code; the K-tests are the named controls (a real write of the member,
//! a real hand-off to unknown code).
use super::*;

/// The payload stored on the request stays unescaped, the `.n` read on
/// the line marked `// SITE_N` has no unmodeled (⊤) receiver, and every
/// `.length` read (of the string a ToString made) has none either. (A
/// read of an undeclared key on the request also sees a builtin function
/// value, so the `.n` verdict itself is not the witness.)
fn proven(src: &str) -> Result<bool, String> {
    let at = src.find("// SITE_N").expect("named site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing SITE_N\n{}", pf.dump_text()));
    }
    let site = sites.iter().all(|s| !s.top);
    let len = pf
        .sites
        .iter()
        .filter(|s| s.key == "length")
        .all(|s| !s.top);
    Ok(site && len && tmpl(&pf, "Payload").escape.is_none())
}

/// The request reaches `req` through a program field, as in FileLoader:
/// the HIR's static fold of a method call on a `new Request` binding
/// (`NativeMethodCall`) is another path.
fn src(setup: &str, read: &str) -> String {
    format!(
        r#"
class Payload {{ n = 1; }}
class Box {{ req: any = null; }}
const payload = new Payload();
const box = new Box();
box.req = new Request("http://x/");
const req: any = box.req;
req.p = payload;
{setup}
console.log({read}.n); // SITE_N
export const keep = 1;
"#
    )
}

/// R1: `req.toString()`, `String(req)`, a template literal and `"" + req`
/// run `Object.prototype.toString` (or ToPrimitive reaching it) on the
/// request: a string, nothing handed to unknown code.
#[test]
fn r1_to_string_of_a_request_is_object_prototype_to_string() {
    for setup in [
        "const s: string = req.toString(); console.log(s.length);",
        "const s = String(req); console.log(s.length);",
        "const s = `${req}`; console.log(s.length);",
        "const s = \"\" + req; console.log(s.length);",
    ] {
        assert_eq!(proven(&src(setup, "req.p")), Ok(true), "{setup}");
    }
}

/// R2: the other `Object.prototype` methods reached on the request's chain
/// run the same model: `valueOf` returns the receiver, the predicates read
/// a key, `toLocaleString` invokes `toString`, `__lookupGetter__` looks
/// the key up.
#[test]
fn r2_other_object_prototype_members_on_a_request() {
    for (setup, read) in [
        ("const v: any = req.valueOf();", "v.p"),
        ("console.log(req.hasOwnProperty(\"p\"));", "req.p"),
        ("console.log(req.propertyIsEnumerable(\"p\"));", "req.p"),
        (
            "console.log(Object.prototype.isPrototypeOf.call({}, req), req.isPrototypeOf({}));",
            "req.p",
        ),
        ("console.log(req.toLocaleString());", "req.p"),
        ("console.log(req.__lookupGetter__(\"p\"));", "req.p"),
    ] {
        assert_eq!(proven(&src(setup, read)), Ok(true), "{setup}");
    }
}

/// R3 (the FileLoader shape): the request held in a program object whose
/// fields are read later (the request points back at it); ToString of
/// the request does not open them.
#[test]
fn r3_file_loader_shape() {
    let s = r#"
class Payload { n = 1; }
class Entry { req: any; payload = new Payload(); }
class Loader {
    load(url: string) {
        const e = new Entry();
        e.req = new Request(url, { method: "GET" });
        e.req.owner = e;
        console.log("loading " + e.req);
        return e;
    }
}
const e = new Loader().load(process.argv[2]);
console.log(e.payload.n); // SITE_N
export const keep = 1;
"#;
    assert_eq!(proven(s), Ok(true));
}

/// K1: `Object.prototype.toString` overwritten by real unknown code runs
/// what was stored: the request reaches it.
#[test]
fn k1_written_object_prototype_to_string_stays_unknown() {
    let setup = "(Object.prototype as any).toString = new Function(process.argv[2])();
        const s = req.toString(); console.log(s);";
    assert_eq!(proven(&src(setup, "req.p")), Ok(false));
}

/// K2: the request handed to real unknown code escapes.
#[test]
fn k2_request_handed_to_unknown_code_escapes() {
    let setup = "new Function(process.argv[2])(req); const s = String(req); console.log(s);";
    assert_eq!(proven(&src(setup, "req.p")), Ok(false));
}
