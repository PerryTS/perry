import { Buffer } from "node:buffer";
import crypto, { webcrypto } from "node:crypto";
import { types as utilTypes } from "node:util";
import * as directTypes from "node:util/types";

function captureArgs() {
  return arguments;
}

const names = [
  typeof directTypes.isArgumentsObject,
  typeof directTypes.isBigIntObject,
  typeof directTypes.isDataView,
  typeof directTypes.isExternal,
  typeof directTypes.isModuleNamespaceObject,
  typeof directTypes.isSymbolObject,
  typeof directTypes.isWeakMap,
  typeof directTypes.isWeakSet,
  typeof directTypes.isFloat16Array,
  typeof directTypes.isKeyObject,
  typeof directTypes.isCryptoKey,
];

console.log("function shapes:", names.join(","));
console.log("namespace identity:", directTypes.isDataView === utilTypes.isDataView);
console.log("arguments true:", directTypes.isArgumentsObject(captureArgs("x", "y")));
console.log("rest array false:", ((...rest: number[]) => directTypes.isArgumentsObject(rest))(1, 2));
console.log("bigint object true:", directTypes.isBigIntObject(Object(1n)));
console.log("bigint primitive false:", directTypes.isBigIntObject(1n));
console.log("symbol object true:", directTypes.isSymbolObject(Object(Symbol("x"))));
console.log("symbol primitive false:", directTypes.isSymbolObject(Symbol("x")));

const backing = new ArrayBuffer(8);
console.log("dataview true:", directTypes.isDataView(new DataView(backing)));
console.log("dataview typed false:", directTypes.isDataView(new Uint8Array(backing)));
console.log("weakmap true:", directTypes.isWeakMap(new WeakMap()));
console.log("weakmap map false:", directTypes.isWeakMap(new Map()));
console.log("weakset true:", directTypes.isWeakSet(new WeakSet()));
console.log("weakset set false:", directTypes.isWeakSet(new Set()));
console.log("external false:", directTypes.isExternal({}));
console.log("module namespace true:", directTypes.isModuleNamespaceObject(directTypes));
console.log("util types object false:", directTypes.isModuleNamespaceObject(utilTypes));
console.log("module namespace plain false:", directTypes.isModuleNamespaceObject({}));
console.log("float16 true:", directTypes.isFloat16Array(new Float16Array(2)));
console.log("float16 uint16 false:", directTypes.isFloat16Array(new Uint16Array(2)));

const keyObject = crypto.createSecretKey(Buffer.from("secret"));
console.log("keyobject true:", directTypes.isKeyObject(keyObject));
console.log("keyobject buffer false:", directTypes.isKeyObject(Buffer.from("secret")));

const cryptoKey = await webcrypto.subtle.importKey(
  "raw",
  Buffer.from("00112233445566778899aabbccddeeff", "hex"),
  "AES-GCM",
  true,
  ["encrypt", "decrypt"],
);
console.log("cryptokey true:", directTypes.isCryptoKey(cryptoKey));
console.log("cryptokey object false:", directTypes.isCryptoKey({}));

const captured = directTypes.isDataView;
console.log("captured dataView true:", captured(new DataView(new ArrayBuffer(1))));
