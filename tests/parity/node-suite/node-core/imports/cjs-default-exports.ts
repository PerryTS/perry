import asyncHooksDefault, * as asyncHooks from "node:async_hooks";
import eventsDefault, * as events from "node:events";
import osDefault, * as os from "node:os";
import pathDefault, * as path from "node:path";
import querystringDefault, * as querystring from "node:querystring";
import urlDefault, * as url from "node:url";
import utilDefault, * as util from "node:util";

type ModuleShape = {
  name: string;
  defaultValue: any;
  namespaceValue: Record<string, any>;
  expectedKey: string;
  expectedLength: number;
  defaultKind: "function" | "object";
};

const modules: ModuleShape[] = [
  {
    name: "async_hooks",
    defaultValue: asyncHooksDefault,
    namespaceValue: asyncHooks,
    expectedKey: "AsyncLocalStorage",
    expectedLength: 0,
    defaultKind: "object",
  },
  {
    name: "events",
    defaultValue: eventsDefault,
    namespaceValue: events,
    expectedKey: "EventEmitter",
    expectedLength: 1,
    defaultKind: "function",
  },
  {
    name: "os",
    defaultValue: osDefault,
    namespaceValue: os,
    expectedKey: "platform",
    expectedLength: 0,
    defaultKind: "object",
  },
  {
    name: "path",
    defaultValue: pathDefault,
    namespaceValue: path,
    expectedKey: "join",
    expectedLength: 0,
    defaultKind: "object",
  },
  {
    name: "querystring",
    defaultValue: querystringDefault,
    namespaceValue: querystring,
    expectedKey: "parse",
    expectedLength: 4,
    defaultKind: "object",
  },
  {
    name: "url",
    defaultValue: urlDefault,
    namespaceValue: url,
    expectedKey: "URL",
    expectedLength: 1,
    defaultKind: "object",
  },
  {
    name: "util",
    defaultValue: utilDefault,
    namespaceValue: util,
    expectedKey: "format",
    expectedLength: 0,
    defaultKind: "object",
  },
];

for (const item of modules) {
  const member = item.defaultValue?.[item.expectedKey];
  const namespaceMember = item.namespaceValue[item.expectedKey];
  const defaultKeys = Object.keys(item.defaultValue ?? {});
  const namespaceKeys = Object.keys(item.namespaceValue ?? {});

  console.log(`${item.name} default kind:`, typeof item.defaultValue, typeof item.defaultValue === item.defaultKind);
  console.log(`${item.name} namespace.default identity:`, item.namespaceValue.default === item.defaultValue);
  console.log(`${item.name} member kind:`, typeof member, typeof namespaceMember, member === namespaceMember);
  console.log(`${item.name} member length:`, member?.length, namespaceMember?.length, item.expectedLength);
  console.log(`${item.name} default keys include member:`, defaultKeys.includes(item.expectedKey));
  console.log(`${item.name} namespace keys include default:`, namespaceKeys.includes("default"));
  console.log(`${item.name} default has own default:`, Object.prototype.hasOwnProperty.call(item.defaultValue, "default"));
}
