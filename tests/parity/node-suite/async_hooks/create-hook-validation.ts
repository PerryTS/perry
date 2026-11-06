// Issue #3089 - async_hooks.createHook rejects nullish options and present
// non-callable hook members while allowing missing or undefined members.
import * as async_hooks from "node:async_hooks";

function probe(scope, label, fn, includeMessage = false) {
  try {
    fn();
    console.log(scope, label, "ok");
  } catch (err) {
    const parts = [scope, label, err.name, err.code || "no-code"];
    if (includeMessage) {
      parts.push(err.message);
    }
    console.log(...parts);
  }
}

probe("options", "undefined", () => async_hooks.createHook(undefined));
probe("options", "null", () => async_hooks.createHook(null));

const allowedOptions = [
  ["number", 0],
  ["boolean", true],
  ["string", "x"],
  ["object", {}],
  ["array", []],
];
for (const [label, value] of allowedOptions) {
  probe("options", label, () => async_hooks.createHook(value));
}

function optionsWith(name, value) {
  const options = {};
  if (name === "init") options.init = value;
  if (name === "before") options.before = value;
  if (name === "after") options.after = value;
  if (name === "destroy") options.destroy = value;
  if (name === "promiseResolve") options.promiseResolve = value;
  return options;
}

for (const name of ["init", "before", "after", "destroy", "promiseResolve"]) {
  probe(name, "undefined", () => async_hooks.createHook(optionsWith(name, undefined)));
  for (const [label, value] of [
    ["null", null],
    ["number", 0],
    ["boolean", true],
    ["string", "x"],
    ["object", {}],
    ["array", []],
  ]) {
    probe(name, label, () => async_hooks.createHook(optionsWith(name, value)), true);
  }
}
