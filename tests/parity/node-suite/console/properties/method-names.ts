const methods = ["log", "warn", "dir", "time", "timeEnd", "timeLog", "trace", "assert", "clear", "count", "countReset", "group", "groupEnd", "table", "debug", "info", "dirxml", "error", "groupCollapsed"];
for (const method of methods) console.log(method + " name:", (console as any)[method].name);
