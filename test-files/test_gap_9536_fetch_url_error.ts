// Regression for #9536: WHATWG fetch accepts a URL object as RequestInfo,
// and transport failures reject with Node's TypeError("fetch failed") whose
// cause carries the underlying system error diagnostics.

async function report(label: string, go: () => Promise<Response>): Promise<void> {
  try {
    const response = await go();
    console.log(label + " -> status " + response.status);
  } catch (error: any) {
    const cause = error.cause;
    console.log(
      label + " -> " + error.name + " " + JSON.stringify(error.message) +
      " cause: " + cause?.name + " " + JSON.stringify(cause?.message) +
      " code=" + cause?.code + " errno=" + cause?.errno +
      " syscall=" + cause?.syscall + " hostname=" + cause?.hostname
    );
  }
}

async function main(): Promise<void> {
  const text = "https://example.invalid/mcp";
  const url = new URL(text);

  await report("string", () => fetch(text, { method: "POST", body: "{}" }));
  await report("URL object", () => fetch(url, { method: "POST", body: "{}" }));
  await report("URL object + Headers", () => fetch(url, {
    method: "POST",
    headers: new Headers({ "X-A": "1" }),
    body: "{}"
  }));
  await report("Request object", () => fetch(new Request(url, {
    method: "POST",
    body: "{}"
  })));
  await report("URL object, GET, signal", () => fetch(url, {
    signal: AbortSignal.timeout(5000)
  }));
}

main();

/*
@covers
crates/perry-codegen/src/expr/logical_collections.rs:
  - lower
crates/perry-runtime/src/object/global_fetch.rs:
  - js_fetch_input_ptr
  - global_this_fetch_thunk
crates/perry-stdlib/src/fetch/abort_bridge.rs:
  - run_request
crates/perry-stdlib/src/fetch/mod.rs:
  - queue_fetch_transport_error
  - js_fetch_with_options
crates/perry-stdlib/src/fetch/transport_error.rs:
  - from_reqwest
  - into_js_bits
*/
