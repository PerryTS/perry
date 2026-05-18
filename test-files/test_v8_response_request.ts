// Test that Response, Request, Headers globals are available in the V8 fallback.
// Build: cargo run --release -- test-files/test_v8_response_request.ts -o /tmp/t_v8_rr --enable-js-runtime
// Run:   /tmp/t_v8_rr

async function main() {
    const res = new Response("hello", { status: 200 });
    console.log(res.status);          // 200
    console.log(await res.text());    // hello

    const req = new Request("http://example.com/", { method: "POST", body: "data" });
    console.log(req.url);             // http://example.com/
    console.log(req.method);          // POST

    const h = new Headers({ "content-type": "text/plain" });
    console.log(h.get("content-type")); // text/plain
}
main();
