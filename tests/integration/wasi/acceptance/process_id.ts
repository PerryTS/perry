// WASI has no process ids and std::process::id() panics there, so every runtime
// path that put a pid in a name or a log line aborted the module. Each section
// below is such a path; the module must run to its end and exit 0.
import * as fs from "node:fs";
import * as traceEvents from "node:trace_events";
import { debuglog } from "node:util";

// The random suffix of mkdtemp.
const dir = fs.mkdtempSync("wasi-process-id-");
console.log("mkdtemp", dir.startsWith("wasi-process-id-"), fs.existsSync(dir));
fs.rmdirSync(dir);

// The "(node:<pid>) Warning: ..." line of process.emitWarning.
process.on("warning", (warning) => console.log("warning", warning.name, warning.message));
process.emitWarning("probe");

// The "<section> <pid>: ..." line of util.debuglog (NODE_DEBUG is set in the
// fixture's .json).
debuglog("wasi_process_id")("probe");

// The trace file the runtime writes when a program that enabled tracing exits.
const tracing = traceEvents.createTracing({ categories: ["node.perf"] });
tracing.enable();
console.log("tracing", tracing.enabled);
