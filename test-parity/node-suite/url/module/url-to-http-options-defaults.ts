import { urlToHttpOptions } from "node:url";

const plain = urlToHttpOptions(new URL("http://example.com/p"));
console.log("plain hostname:", plain.hostname);
console.log("plain port:", plain.port);
console.log("plain path:", plain.path);
console.log("plain auth:", plain.auth);
console.log("plain protocol:", plain.protocol);

const noPath = urlToHttpOptions(new URL("https://example.com"));
console.log("noPath path:", noPath.path);

const onlyUser = urlToHttpOptions(new URL("http://u@example.com/"));
console.log("onlyUser auth:", onlyUser.auth);
