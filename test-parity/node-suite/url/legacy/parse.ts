import { parse } from "node:url";

const legacy = parse("https://user:pw@example.com:8080/p?q=1#h", true);
console.log("host:", legacy.host);
console.log("hostname:", legacy.hostname);
console.log("port:", legacy.port);
console.log("pathname:", legacy.pathname);
console.log("search:", legacy.search);
console.log("query:", (legacy.query as Record<string, string>).q);
console.log("hash:", legacy.hash);
console.log("auth:", legacy.auth);
