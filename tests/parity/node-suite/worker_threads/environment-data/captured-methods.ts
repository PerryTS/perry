import { getEnvironmentData, setEnvironmentData } from "node:worker_threads";

const namespace = process.getBuiltinModule("node:worker_threads");
const read = namespace.getEnvironmentData;
const write = namespace.setEnvironmentData;
const key = {};
const value = { count: 1 };
setEnvironmentData(key, value);
console.log("identity:", read(key) === value, read({}));
console.log("write:", write("shared", 42), getEnvironmentData("shared"));
write(NaN, 7);
write(-0, 8);
console.log("keys:", read(NaN), read(0));
write(undefined, 9);
console.log("omitted key:", read());
write();
write("shared");
console.log("deleted:", read(), getEnvironmentData("shared"));
