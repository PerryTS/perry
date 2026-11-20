import { createServer } from "node:http2";

const listener = () => {};
const bare = createServer();
const direct = createServer(listener);
const options = createServer({}, listener);

console.log("bare request listeners:", bare.listenerCount("request"));
console.log("direct request listeners:", direct.listenerCount("request"));
console.log("options request listeners:", options.listenerCount("request"));
