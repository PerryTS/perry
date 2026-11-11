import ModuleDefault, * as ModuleNS from "node:module";
import { createRequire } from "node:module";

function line(name, value) {
  console.log(name + ": " + String(value));
}

line("default-type", typeof ModuleDefault);
line("namespace-default-type", typeof ModuleNS.default);
line("default-eq-namespace-default", ModuleDefault === ModuleNS.default);
line("createRequire-type", typeof createRequire);
line("createRequire-length", createRequire.length);

const req = createRequire(import.meta.url);

line("require-type", typeof req);
line("require-name", req.name);
line("require-length", req.length);
line("resolve-type", typeof req.resolve);
line("resolve-name", req.resolve.name);
line("resolve-length", req.resolve.length);
line("resolve-paths-type", typeof req.resolve.paths);
line("resolve-fs", req.resolve("fs"));
line("resolve-node-fs", req.resolve("node:fs"));
line("paths-fs", req.resolve.paths("fs"));
line("cache-type", typeof req.cache);
line("cache-same", req.cache === req.cache);
line("cache-keys-len", Object.keys(req.cache).length);
line("extensions-type", typeof req.extensions);
line("extensions-keys", Object.keys(req.extensions).sort().join(","));
line("ext-js-type", typeof req.extensions[".js"]);
line("main-in", Object.prototype.hasOwnProperty.call(req, "main"));
line("main-value", req.main);
line("fs-readFileSync-type", typeof req("node:fs").readFileSync);
line("path-sep", req("node:path").sep);

try {
  createRequire(123);
} catch (err) {
  line("bad-create-code", err.code);
  line("bad-create-name", err.name);
}

try {
  req(123);
} catch (err) {
  line("bad-require-code", err.code);
  line("bad-require-name", err.name);
}

try {
  req("");
} catch (err) {
  line("empty-require-code", err.code);
  line("empty-require-name", err.name);
}

try {
  req.resolve(123);
} catch (err) {
  line("bad-resolve-code", err.code);
  line("bad-resolve-name", err.name);
}
