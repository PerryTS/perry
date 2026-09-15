//! #10274: a dynamic HeadersInit in a literal RequestInit needs conversion.

mod support;

const SOURCE: &str = r#"
const dump = (h: any) => { const out: string[] = []; h.forEach((v: string, k: string) => out.push(k + "=" + v)); return out.sort() }
const proxy = new Proxy({ "x-a": "1" }, {})
console.log("direct", JSON.stringify(dump(new Headers(proxy as any))))
console.log("request", JSON.stringify(dump(new Request("https://x.dev", { headers: proxy as any }).headers)))
function make(headers: any) { return new Request("https://x.dev", { headers }).headers; }
function init(headers: any): any { return { headers }; }
console.log("parameter", JSON.stringify(dump(make(proxy))))
console.log("runtime init", JSON.stringify(dump(new Request("https://x.dev", init(proxy)).headers)))
const trapped = new Proxy({ "authorization": "secret", "x-hidden": "hidden" }, {
  ownKeys() { return ["authorization"]; },
  get(t: any, k: any) { return k === "authorization" ? "Bearer token" : t[k]; }
});
console.log("traps", JSON.stringify(dump(make(trapped))))
console.log("record", JSON.stringify(dump(make({ "x-a": "1" }))))
console.log("pairs", JSON.stringify(dump(make([["x-a", "1"], ["x-a", "2"]]))))
console.log("handle", JSON.stringify(dump(make(new Headers({ "x-a": "1" })))))
console.log("literal", JSON.stringify(dump(new Request("https://x.dev", { headers: { "x-a": "1" } }).headers)))
const source = new Headers({ "x-a": "1" })
const copied = make(source)
source.set("x-b", "2")
copied.set("x-c", "3")
console.log("independent", JSON.stringify(dump(source)), JSON.stringify(dump(copied)), copied !== source)
console.log("absent", JSON.stringify(dump(make(undefined))))
try { make(null); console.log("null accepted") } catch (e) { console.log("null", e instanceof TypeError) }
"#;

#[test]
fn request_converts_dynamic_headers_init() {
    assert_eq!(
        support::compile_and_run(SOURCE),
        concat!(
            "direct [\"x-a=1\"]\n",
            "request [\"x-a=1\"]\n",
            "parameter [\"x-a=1\"]\n",
            "runtime init [\"x-a=1\"]\n",
            "traps [\"authorization=Bearer token\"]\n",
            "record [\"x-a=1\"]\n",
            "pairs [\"x-a=1, 2\"]\n",
            "handle [\"x-a=1\"]\n",
            "literal [\"x-a=1\"]\n",
            "independent [\"x-a=1\",\"x-b=2\"] [\"x-a=1\",\"x-c=3\"] true\n",
            "absent []\n",
            "null true\n",
        )
    );
}
