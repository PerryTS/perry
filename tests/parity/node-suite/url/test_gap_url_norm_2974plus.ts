// #2974 construction normalization, #2976 urlToHttpOptions fields, #2975 windows file-URL paths
import { fileURLToPath, pathToFileURL, urlToHttpOptions } from "node:url";

// ===== #2974: normalize scheme and host during URL construction =====
for (const s of [
  "HTTP://EXAMPLE.COM:80/A",
  "https://bücher.example/p",
  "https://XN--BCHER-KVA.EXAMPLE/p",
  "ftp://EXAMPLE.COM:21/a",
]) {
  const u = new URL(s);
  console.log(
    JSON.stringify([u.href, u.protocol, u.host, u.hostname, u.port, u.origin]),
  );
}

// ===== #2976: urlToHttpOptions fields + validation =====
const u = new URL("https://u%20ser:p%40w@example.com:8080/p/a?q=1#frag");
(u as any).extra = "own";
console.log(JSON.stringify(Object.keys(urlToHttpOptions(u))));
console.log(JSON.stringify(urlToHttpOptions(u)));
console.log(JSON.stringify(urlToHttpOptions(new URL("https://example.com"))));
console.log(
  JSON.stringify(urlToHttpOptions(new URL("https://user@example.com/")).auth),
);
try {
  urlToHttpOptions("https://example.com/p" as any);
  console.log("no-throw");
} catch (e: any) {
  console.log("THROW", e.code);
}

// ===== #2975: windows option in file URL path helpers =====
console.log(JSON.stringify(fileURLToPath("file:///C:/path/with%20space", { windows: true })));
console.log(JSON.stringify(fileURLToPath("file://server/share/f.txt", { windows: true })));
try {
  fileURLToPath("file:///tmp/a%5Cb", { windows: true });
  console.log("no-throw-5c");
} catch (e: any) {
  console.log("THROW", e.code);
}
console.log(JSON.stringify(fileURLToPath("file:///tmp/a%5Cb", { windows: false })));
console.log(JSON.stringify(pathToFileURL("C:\\path\\a b", { windows: true }).href));
console.log(JSON.stringify(pathToFileURL("\\\\server\\share\\f.txt", { windows: true }).href));
