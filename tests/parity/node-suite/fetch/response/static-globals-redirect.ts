function log(label: string, value: unknown) {
  console.log(label, value);
}

function logSync(label: string, fn: () => unknown) {
  try {
    console.log(label, fn());
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message.split("\n")[0]);
  }
}

async function logAsync(label: string, fn: () => Promise<unknown>) {
  try {
    console.log(label, await fn());
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message.split("\n")[0]);
  }
}

const g = globalThis as any;
const {
  Blob: GlobalBlob,
  Headers: GlobalHeaders,
  Request: GlobalRequest,
  Response: GlobalResponse,
} = g;
const bareConstructors: Record<string, any> = {
  Blob,
  Headers,
  Request,
  Response,
};

for (const name of ["Blob", "Headers", "Request", "Response"]) {
  const C = g[name];
  log(`${name} global typeof`, typeof C);
  log(`${name} global same bare`, C === bareConstructors[name]);
  log(`${name} prototype constructor`, C.prototype.constructor === C);
}

log("destructured Blob text", typeof new GlobalBlob(["x"]).text);
log("destructured Headers get", new GlobalHeaders({ A: "b" }).get("a"));
log(
  "destructured Request method",
  new GlobalRequest("http://example.com/path", { method: "POST" }).method,
);

const responseJson = GlobalResponse.json;
const responseRedirect = GlobalResponse.redirect;
const responseError = GlobalResponse.error;

function callRedirect(args: readonly [string] | readonly [string, number]) {
  return args.length === 1
    ? responseRedirect(args[0])
    : responseRedirect(args[0], args[1]);
}

log(
  "Response statics typeof",
  `error:${typeof responseError},json:${typeof responseJson},redirect:${typeof responseRedirect}`,
);

await logAsync("Response.json rebound", async () => {
  const r = responseJson({ ok: true });
  return `${r.status}|${r.headers.get("content-type")}|${await r.text()}`;
});

for (const args of [
  ["http://example.com/a"],
  ["http://example.com/b", 301],
  ["http://example.com/c", 302],
  ["http://example.com/d", 303],
  ["http://example.com/e", 307],
  ["http://example.com/f", 308],
  ["https://example.com/a b", 302],
  ["http://example.com/g", 302.9],
  ["http://example.com/h", 65837],
] as const) {
  logSync(`Response.redirect ok ${JSON.stringify(args)}`, () => {
    const r = callRedirect(args);
    return `${r.status}|${JSON.stringify(r.statusText)}|${r.headers.get("location")}|${r.type}|${r.redirected}|${r.ok}`;
  });
}

for (const args of [
  ["http://example.com/bad", 200],
  ["http://example.com/bad", 300],
  ["http://example.com/bad", 304],
  ["http://example.com/bad", 400],
  ["http://example.com/bad", 99999],
  ["http://example.com/bad", -1],
  ["not a url", 302],
] as const) {
  logSync(`Response.redirect invalid ${JSON.stringify(args)}`, () => {
    callRedirect(args);
    return "ok";
  });
}
