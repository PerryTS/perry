// Issue #1691 — an inline `new Request(...)` / `new Response(...)` whose
// result is consumed immediately (never bound to a local) must still set the
// `uses_fetch` compile flag. Before the fix, only the var-decl form
// (`const r = new Request(...)`) set it, so the auto-optimize build stripped
// the fetch / http-client feature and the link failed with
// `Undefined symbols: _js_request_new` / `_js_request_text` / ...
//
// This mirrors the issue's repro: inline, no variable assignment.
async function main() {
  console.log("req:", await new Request("http://x/", { method: "POST", body: "hi" }).text());
  console.log("res:", await new Response("world").text());
}
main();
