import { getEnvironmentData, setEnvironmentData } from "node:worker_threads";

const key = { key: true };
const value = { nested: { count: 1 } };

setEnvironmentData(key, value);
console.log("same key:", getEnvironmentData(key) === value);
console.log("different key:", getEnvironmentData({ key: true }));

value.nested.count = 2;
console.log("live value:", getEnvironmentData(key)?.nested.count);

setEnvironmentData(key, undefined);
console.log("deleted:", getEnvironmentData(key));
