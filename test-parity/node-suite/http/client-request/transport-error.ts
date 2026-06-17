// A failed `http.request` / `http.get` hands `request.on('error')` a real
// Node system `Error`, not a bare string: `.code` (`ECONNREFUSED` /
// `ENOTFOUND`), `.syscall` (`connect` / `getaddrinfo`), `.errno` (the
// libuv-negative number) and a `${syscall} ${CODE} ${detail}` message.
// Libraries branch on `err.code === 'ECONNREFUSED'`, so the prior bare-string
// surface broke every one of them. errno is read from the same OS error Node
// sees (this suite diffs Perry vs node on the one host), so it matches.
import http from "node:http";

function describe(label: string, e: any) {
  console.log(
    label,
    [
      typeof e,
      e && e.name,
      e && e.message,
      e && e.code,
      e && e.syscall,
      e && e.errno,
      e instanceof Error,
    ].join("|"),
  );
}

// Port 1 is reserved/unused — a connect there is refused immediately.
const refused = http.get({ host: "127.0.0.1", port: 1, path: "/" }, () => {});
refused.on("error", (e: any) => {
  describe("refused:", e);

  // A DNS lookup failure: `.invalid` is reserved (RFC 6761) and never resolves.
  const notfound = http.get(
    { host: "does-not-exist.invalid", port: 80, path: "/" },
    () => {},
  );
  notfound.on("error", (e2: any) => {
    describe("notfound:", e2);
  });
});

setTimeout(() => {}, 2000);
