import { Recoverable, REPLServer, start, writer } from "node:repl";

for (const value of [start, writer, REPLServer, Recoverable]) {
  console.log(value.name, value.length);
}
