function own(obj: unknown, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

function errorLine(label: string, fn: () => unknown): void {
  try {
    fn();
    console.log(label + ": no throw");
  } catch (err) {
    const e = err as Error;
    console.log(label + ":", e.name + ":" + String(e.message).split("\n")[0]);
  }
}

const order: string[] = [];
const nested = JSON.parse('{"a":1,"b":{"c":2},"d":3}', function (key, value) {
  order.push(key + ":" + (value === null ? "null" : typeof value));
  if (key === "c") {
    return 4;
  }
  return value;
});
console.log("post order:", order.join("|"));
console.log("nested value:", nested.b.c);

const objectDelete = JSON.parse('{"a":1,"b":2,"c":3}', function (key, value) {
  return key === "b" ? undefined : value;
});
console.log(
  "object delete:",
  own(objectDelete, "b"),
  Object.keys(objectDelete).join("|"),
  JSON.stringify(objectDelete),
);

const arrayDelete = JSON.parse("[1,2,3]", function (key, value) {
  return key === "1" ? undefined : value;
});
console.log(
  "array delete:",
  arrayDelete.length,
  own(arrayDelete, "1"),
  1 in arrayDelete,
  Object.keys(arrayDelete).join("|"),
  JSON.stringify(arrayDelete),
);

const protoSeen: unknown[] = [];
const reread = JSON.parse('{"a":1,"b":2}', function (key, value) {
  if (key === "a") {
    delete this.b;
    Object.setPrototypeOf(this, { b: 3 });
  }
  if (key === "b") {
    protoSeen.push(value);
  }
  return value;
});
console.log(
  "holder reread:",
  protoSeen.join("|"),
  reread.b,
  own(reread, "b"),
  Object.getPrototypeOf(reread).b,
);

let rootOwn = false;
const rootResult = JSON.parse('{"x":1}', function (key, value) {
  if (key === "") {
    rootOwn = own(this, "");
    return undefined;
  }
  return value;
});
console.log("root holder:", rootOwn, rootResult === undefined);

errorLine("getter abrupt", () =>
  JSON.parse('{"a":1,"b":2}', function (key, value) {
    if (key === "a") {
      Object.defineProperty(this, "b", {
        get() {
          throw new Error("boom-get");
        },
        enumerable: true,
        configurable: true,
      });
    }
    return value;
  }),
);

const nonConfigDelete = JSON.parse('{"locked":1}', function (key, value) {
  if (key === "locked") {
    Object.defineProperty(this, "locked", {
      value,
      writable: true,
      enumerable: true,
      configurable: false,
    });
    return undefined;
  }
  return value;
});
const nonConfigDeleteDesc = Object.getOwnPropertyDescriptor(nonConfigDelete, "locked");
console.log(
  "nonconfig delete:",
  own(nonConfigDelete, "locked"),
  nonConfigDelete.locked,
  Object.keys(nonConfigDelete).join("|"),
  nonConfigDeleteDesc?.configurable,
);

const nonConfigDefine = JSON.parse('{"locked":1}', function (key, value) {
  if (key === "locked") {
    Object.defineProperty(this, "locked", {
      value,
      writable: false,
      enumerable: true,
      configurable: false,
    });
    return 2;
  }
  return value;
});
const nonConfigDefineDesc = Object.getOwnPropertyDescriptor(nonConfigDefine, "locked");
console.log(
  "nonconfig define:",
  nonConfigDefine.locked,
  nonConfigDefineDesc?.writable,
  nonConfigDefineDesc?.configurable,
);
