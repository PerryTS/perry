import * as net from "node:net";

// #2013: net.createServer(options) requires the first argument to be a function
// (the connection listener) or an object (the options bag). A number or boolean
// throws TypeError [ERR_INVALID_ARG_TYPE].
const badFirst: any[] = [5, true];

for (const value of badFirst) {
  try {
    net.createServer(value);
    console.log("createServer", JSON.stringify(value), "=> NO THROW");
  } catch (err: any) {
    console.log("createServer", JSON.stringify(value), "=>", err.name, err.code, "|", err.message);
  }
}

// A leading non-object also throws when a listener follows it.
try {
  net.createServer(5 as any, () => {});
  console.log("createServer(5, fn) => NO THROW");
} catch (err: any) {
  console.log("createServer(5, fn) =>", err.name, err.code);
}

// Accepted forms: a listener function, an options object, or nothing.
console.log("createServer(fn):", typeof net.createServer(() => {}));
console.log("createServer({}):", typeof net.createServer({}));
console.log("createServer(fn, fn):", typeof net.createServer(() => {}, () => {}));
console.log("createServer():", typeof net.createServer());
