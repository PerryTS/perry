import {
  _errnoException,
  _exceptionWithHostPort,
  _extend,
} from "node:util";

const target = { a: 1 };
const source = { b: 2, a: 3 };
console.log("extend:", _extend(target, source) === target, JSON.stringify(target));
console.log("extend-null-source:", JSON.stringify(_extend({ a: 1 }, null)));
console.log("extend-primitive-source:", JSON.stringify(_extend({ a: 1 }, "hi")));

function showError(label: string, value: unknown) {
  const err = value as NodeJS.ErrnoException & { address?: string; port?: number };
  console.log(
    label + ":",
    err.name,
    err.message,
    err.code,
    err.errno,
    err.syscall,
    err.address || "",
    err.port || "",
  );
}

showError("errno-known", _errnoException(-2, "open", "custom path"));
showError("errno-unknown", _errnoException(-9999, "open", "thing"));
showError(
  "host-port",
  _exceptionWithHostPort(-111, "connect", "127.0.0.1", 443, "localhost"),
);
showError("path", _exceptionWithHostPort(-2, "listen", "/tmp/sock"));

try {
  _errnoException(2, "open");
} catch (err) {
  const e = err as NodeJS.ErrnoException;
  console.log("errno-positive:", e.name, e.code);
}
