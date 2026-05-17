export const SAFE_DATA = Object.freeze({
  label: "safe",
  count: 7,
  nested: Object.freeze({
    flag: true,
    text: "nested",
  }),
});

export const MUTABLE_DATA = {
  value: "before",
};

export function mutateMutable() {
  MUTABLE_DATA.value = "after";
  return MUTABLE_DATA.value;
}

export function readMutable(value) {
  return value.value;
}

let accessorReads = 0;
export const ACCESSOR_DATA = Object.freeze(
  Object.defineProperty({}, "value", {
    enumerable: true,
    configurable: false,
    get() {
      accessorReads++;
      return `accessor:${accessorReads}`;
    },
  }),
);

export function readAccessorTwice(value) {
  return `${value.value},${value.value}`;
}

const customProto = {
  inherited: "from-proto",
};
export const CUSTOM_PROTO_DATA = Object.freeze(
  Object.assign(Object.create(customProto), {
    own: "own",
  }),
);

export function readCustomProto(value) {
  return `${value.own}:${value.inherited}`;
}

let proxyReads = 0;
function proxyTarget() {
  return "proxy-target";
}
export const PROXY_DATA = new Proxy(proxyTarget, {
  get(target, property, receiver) {
    if (property === "value") {
      proxyReads++;
      return `proxy:${proxyReads}`;
    }
    return Reflect.get(target, property, receiver);
  },
});

export function readProxyTwice(value) {
  proxyReads = 0;
  return `${value.value},${value.value}`;
}

export const ARRAY_DATA = Object.freeze(["a", "b"]);

export function readArray(value) {
  return `${value.length}:${value[0]}:${value[1]}`;
}

export function FUNCTION_DATA() {
  return "function:ok";
}

export const PROMISE_DATA = Promise.resolve("promise:ok");

const hardeningSymbol = Symbol.for("perry.exportHardening");
export const SYMBOL_DATA = Object.freeze({
  [hardeningSymbol]: "symbol:ok",
  value: "visible",
});

export function readSymbol(value) {
  return value[hardeningSymbol];
}

const originalObject = Object;
const tamperedPrototype = {
  inherited: "tampered-proto",
};
const tamperedIntrinsicsData = originalObject.create(tamperedPrototype);
originalObject.defineProperty(tamperedIntrinsicsData, "own", {
  value: "own",
  enumerable: true,
  writable: false,
  configurable: false,
});

export const TAMPERED_INTRINSICS_DATA = tamperedIntrinsicsData;

globalThis.Object = {
  prototype: tamperedPrototype,
  isFrozen() {
    return true;
  },
};

export function readTamperedIntrinsics(value) {
  value.extra = "added";
  return `${value.own}:${value.inherited}:${value.extra}`;
}
