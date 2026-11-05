function line(name, value) {
  console.log(name + ": " + String(value));
}

line("require-type", typeof require);
line("require-name", require.name);
line("require-length", require.length);
line("resolve-type", typeof require.resolve);
line("resolve-name", require.resolve.name);
line("resolve-length", require.resolve.length);
line("resolve-paths-type", typeof require.resolve.paths);
line("resolve-fs", require.resolve("fs"));
line("resolve-node-fs", require.resolve("node:fs"));
line("paths-fs", require.resolve.paths("fs"));
line("cache-type", typeof require.cache);
line("cache-same", require.cache === require.cache);
line("extensions-type", typeof require.extensions);
line("extensions-keys", Object.keys(require.extensions).sort().join(","));
line("ext-js-type", typeof require.extensions[".js"]);
line("main-in", Object.prototype.hasOwnProperty.call(require, "main"));
line("main-type", typeof require.main);
line("fs-readFileSync-type", typeof require("node:fs").readFileSync);
line("path-sep", require("node:path").sep);

try {
  require(123);
} catch (err) {
  line("bad-require-code", err.code);
  line("bad-require-name", err.name);
}

try {
  require("");
} catch (err) {
  line("empty-require-code", err.code);
  line("empty-require-name", err.name);
}

try {
  require.resolve(123);
} catch (err) {
  line("bad-resolve-code", err.code);
  line("bad-resolve-name", err.name);
}
