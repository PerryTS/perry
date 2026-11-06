import { urlToHttpOptions } from "node:url";

const opts = urlToHttpOptions(new URL("https://user:pw@example.com:8080/p?q=1"));
console.log("hostname:", opts.hostname);
console.log("port:", opts.port);
console.log("path:", opts.path);
console.log("auth:", opts.auth);
console.log("protocol:", opts.protocol);
