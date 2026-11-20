import * as worker_threads from "node:worker_threads";
import { getEnvironmentData, setEnvironmentData } from "node:worker_threads";

console.log("namespace set typeof:", typeof worker_threads.setEnvironmentData);
console.log("namespace get typeof:", typeof worker_threads.getEnvironmentData);
console.log("named set typeof:", typeof setEnvironmentData);
console.log("named get typeof:", typeof getEnvironmentData);
console.log("missing:", getEnvironmentData("missing-key"));
console.log("set return:", setEnvironmentData("env-key", "env-value"));
console.log("named get:", getEnvironmentData("env-key"));
console.log("namespace get:", worker_threads.getEnvironmentData("env-key"));
worker_threads.setEnvironmentData("env-key", "updated");
console.log("updated:", getEnvironmentData("env-key"));
worker_threads.setEnvironmentData("env-key", undefined);
console.log("deleted:", getEnvironmentData("env-key"));
