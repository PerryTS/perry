// Integration test for Fastify (issue #174). Runs a small server that
// scripts/run_fastify_tests.sh launches in the background, curls, and
// asserts the response bodies for each route. Port is read from argv
// so the harness can pick a free port to avoid CI conflicts.
import fastify from "fastify";

const port = parseInt(process.argv[2] || "3456");

const app = fastify();

app.get("/hello", async (_request, _reply) => {
  return { hello: "world" };
});

app.get("/users/:id", async (request, _reply) => {
  const { id } = request.params;
  return { id: id, name: "User " + id };
});

app.post("/echo", async (request, reply) => {
  reply.code(201);
  return { received: request.body };
});

app.get("/throw-sync", (_request, _reply) => {
  throw new Error("sync route boom");
});

app.get("/throw-async", async (_request, _reply) => {
  throw new Error("async route boom");
});

// Issue #1070: `instanceof`-narrowed property access on the error value
// inside a `setErrorHandler(async (err, req, reply) => …)` callback
// previously printed `0`. Pre-fix the first arrow param (`err`) was tagged
// as a fastify Request native instance, so `err.<field>` lowered to a
// NativeMethodCall whose unknown-method fall-through emits `0.0`.
class ProblemError extends Error {
  constructor(public readonly problem: { title: string; status: number }) {
    super(problem.title);
  }
}
app.setErrorHandler(async (err, _req, reply) => {
  if (err instanceof ProblemError) {
    void reply.code(err.problem.status).send(err.problem);
    return;
  }
  // Defer to the default fastify envelope shape for everything else, so
  // the pre-existing /throw-sync and /throw-async parity assertions keep
  // their `{"statusCode":500,"error":"Internal Server Error","message":…}`
  // bodies after this test added a user handler to the app.
  void reply.code(500).send({
    statusCode: 500,
    error: "Internal Server Error",
    message: err.message,
  });
});
app.get("/throw-problem", async (_request, _reply) => {
  throw new ProblemError({ title: "Bad Request", status: 400 });
});

app.listen({ port: port }, () => {
  // Sentinel line the harness waits for before starting curl assertions.
  console.log("ready port=" + port);
});

/*
@covers
crates/perry-stdlib/src/framework/multipart.rs:
  - js_multipart_parse
  - js_multipart_parse_with_sizes
crates/perry-stdlib/src/framework/request.rs:
  - js_http_request_body_length
  - js_http_request_content_type
  - js_http_request_has_header
  - js_http_request_headers_all
  - js_http_request_id
  - js_http_request_is_method
  - js_http_request_query_all
  - js_http_request_query_param
crates/perry-stdlib/src/framework/response.rs:
  - js_http_respond_error
  - js_http_respond_html
  - js_http_respond_json
  - js_http_respond_not_found
  - js_http_respond_redirect
  - js_http_respond_status_text
  - js_http_respond_text
  - js_http_respond_with_headers
crates/perry-stdlib/src/framework/server.rs:
  - js_http_request_body
  - js_http_request_header
  - js_http_request_method
  - js_http_request_path
  - js_http_request_query
  - js_http_respond
  - js_http_server_accept
  - js_http_server_accept_v2
  - js_http_server_close
  - js_http_server_create
*/
