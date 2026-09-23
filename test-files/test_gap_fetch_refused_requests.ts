// Every request shape the fetch transport refuses to build rejects the way
// Node's undici does. Before tokio lane G these fell back to a reqwest future:
// the error text was reqwest's, and embedded credentials / CONNECT / TRACE /
// a malformed method were SENT (reqwest is more permissive than undici).
// Nothing listens on 127.0.0.1:39123, so nothing here needs a network.

async function report(label: string, go: () => Promise<Response>): Promise<void> {
  try {
    const response = await go();
    console.log(label + " -> status " + response.status);
  } catch (error: any) {
    const cause = error?.cause;
    console.log(
      label + " -> " + error?.name + " " + JSON.stringify(error?.message) +
      " cause: " + cause?.name + " " + JSON.stringify(cause?.message) +
      " code=" + cause?.code
    );
  }
}

async function main(): Promise<void> {
  await report("ftp scheme", () => fetch("ftp://127.0.0.1:39123/x"));
  await report("file scheme", () => fetch("file:///etc/hosts"));
  await report("credentials", () => fetch("http://user:pass@127.0.0.1:39123/x"));
  await report("not a url", () => fetch("not-a-url"));
  await report("CONNECT", () => fetch("http://127.0.0.1:39123/x", { method: "CONNECT" }));
  await report("TRACE", () => fetch("http://127.0.0.1:39123/x", { method: "TRACE" }));
  await report("bad method", () => fetch("http://127.0.0.1:39123/x", { method: "BAD METHOD" }));
  // Control: a request the transport DOES build, failing at connect.
  await report("refused", () => fetch("http://127.0.0.1:39123/x"));
}

main();

/*
@covers
crates/perry-stdlib/src/fetch/turnloop_bridge.rs:
  - dispatch
crates/perry-stdlib/src/fetch/transport_error.rs:
  - for_declined
  - into_js_bits
crates/perry-stdlib/src/turnloop_client/mod.rs:
  - submit
*/
