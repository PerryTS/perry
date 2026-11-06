import { Console } from "node:console";

const methods = ["log", "warn", "dir", "time", "timeEnd", "timeLog", "trace", "assert", "clear", "count", "countReset", "group", "groupEnd", "table", "debug", "info", "dirxml", "error", "groupCollapsed"];
console.log("Console type:", typeof Console);
for (const method of methods) console.log(method + " type:", typeof (console as any)[method]);
