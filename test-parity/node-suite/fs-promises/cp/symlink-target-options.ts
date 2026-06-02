import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_cp_symlink_target_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const base = ROOT + "/default";
await fsp.mkdir(base, { recursive: true });
await fsp.writeFile(base + "/target.txt", "hello");
await fsp.mkdir(base + "/from");
await fsp.symlink("../target.txt", base + "/from/rel_link");
await fsp.symlink(base + "/target.txt", base + "/from/abs_link");
await fsp.cp(base + "/from", base + "/to", { recursive: true });
console.log("promises cp relative symlink resolved:", await fsp.readlink(base + "/to/rel_link") === base + "/target.txt");
console.log("promises cp abs symlink preserved target:", await fsp.readlink(base + "/to/abs_link") === base + "/target.txt");
await fsp.rm(base + "/from", { recursive: true, force: true });
console.log("promises cp copied symlink survives source removal:", await fsp.readFile(base + "/to/rel_link", "utf8"));

const verbatimBase = ROOT + "/verbatim";
await fsp.mkdir(verbatimBase, { recursive: true });
await fsp.writeFile(verbatimBase + "/target.txt", "hello");
await fsp.mkdir(verbatimBase + "/from");
await fsp.symlink("../target.txt", verbatimBase + "/from/rel_link");
await fsp.symlink(verbatimBase + "/target.txt", verbatimBase + "/from/abs_link");
await fsp.cp(verbatimBase + "/from", verbatimBase + "/to", { recursive: true, verbatimSymlinks: true });
console.log("promises cp verbatim symlink target:", await fsp.readlink(verbatimBase + "/to/rel_link"));
