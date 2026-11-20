// HWS companion: url_engine
// HWS hook: js_runtime_install_url_engine
// HWS prefixes: url::|perry_runtime::url::
const u: any = new URL("https://example.com/first?a=1");
u.href = "http://user:pass@example.org:81/next?b=2#old";
u.protocol = "https:"; u.hostname = "other.example"; u.port = "8443";
u.pathname = "/new path"; u.search = "?c=3&c=4"; u.hash = "end";
console.log(u.href, u.origin, String(u));
const p: any = u.searchParams;
p.set("c", "5"); p.append("d", "a b"); p.delete("missing");
console.log(p.toString(), u.search, p.get("c"), p.has("d"));
const set = URLSearchParams.prototype.set;
set.call(p, "c", "6"); console.log(p.toString());
