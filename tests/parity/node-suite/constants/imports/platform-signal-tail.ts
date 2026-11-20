import constantsDefault from "node:constants";
import * as constantsNs from "node:constants";
import os from "node:os";
import process from "node:process";

const platformSignals =
  process.platform === "linux"
    ? ["SIGSTKFLT", "SIGPOLL", "SIGPWR"]
    : process.platform === "darwin"
      ? ["SIGINFO"]
      : [];

for (const name of platformSignals) {
  const defaultValue = (constantsDefault as Record<string, unknown>)[name];
  const nsValue = (constantsNs as Record<string, unknown>)[name];
  const osValue = (os.constants.signals as Record<string, unknown>)[name];
  console.log(`${name}:`, defaultValue, nsValue, osValue, defaultValue === osValue);
}

const defaultKeys = Object.keys(constantsDefault).sort();
const nsKeys = Object.keys(constantsNs)
  .filter((name) => name !== "default")
  .sort();
const defaultOnly = defaultKeys.filter((name) => !nsKeys.includes(name));
const nsOnly = nsKeys.filter((name) => !defaultKeys.includes(name));

console.log("platform signal count:", platformSignals.length);
console.log(
  "platform signals present:",
  platformSignals.every(
    (name) =>
      defaultKeys.includes(name) &&
      nsKeys.includes(name) &&
      (constantsDefault as any)[name] === (os.constants.signals as any)[name],
  ),
);
console.log("default only keys:", defaultOnly.join(",") || "none");
console.log("namespace only keys:", nsOnly.join(",") || "none");
