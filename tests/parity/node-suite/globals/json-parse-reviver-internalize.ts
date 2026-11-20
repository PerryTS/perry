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

const inheritedKey = "__perry_json_reviver_inherited__";
const inheritedHadOwn = own(Object.prototype, inheritedKey);
const inheritedPrevious = (Object.prototype as any)[inheritedKey];
const inheritedSeen: unknown[] = [];
try {
  const inheritedObject = JSON.parse(`{"a":1,"${inheritedKey}":2}`, function (key, value) {
    if (key === "a") {
      delete this[inheritedKey];
      (Object.prototype as any)[inheritedKey] = 99;
    }
    if (key === inheritedKey) {
      inheritedSeen.push(value);
    }
    return value;
  });
  console.log(
    "object prototype reread:",
    inheritedSeen.join("|"),
    own(inheritedObject, inheritedKey),
    inheritedObject[inheritedKey],
  );
} finally {
  if (inheritedHadOwn) {
    (Object.prototype as any)[inheritedKey] = inheritedPrevious;
  } else {
    delete (Object.prototype as any)[inheritedKey];
  }
}

const inheritedArrayHadOwn = own(Object.prototype, "1");
const inheritedArrayPrevious = (Object.prototype as any)["1"];
const inheritedArraySeen: unknown[] = [];
try {
  const inheritedArray = JSON.parse("[1,2,3]", function (key, value) {
    if (key === "0") {
      delete this[1];
      (Object.prototype as any)["1"] = 88;
    }
    if (key === "1") {
      inheritedArraySeen.push(value);
    }
    return value;
  });
  console.log(
    "array prototype reread:",
    inheritedArraySeen.join("|"),
    inheritedArray.length,
    own(inheritedArray, "1"),
    inheritedArray[1],
  );
} finally {
  if (inheritedArrayHadOwn) {
    (Object.prototype as any)["1"] = inheritedArrayPrevious;
  } else {
    delete (Object.prototype as any)["1"];
  }
}

const inheritedArrayAccessorPrevious = Object.getOwnPropertyDescriptor(Object.prototype, "1");
const inheritedArrayAccessorSeen: unknown[] = [];
try {
  const inheritedArrayAccessor = JSON.parse("[1,2,3]", function (key, value) {
    if (key === "0") {
      delete this[1];
      Object.defineProperty(Object.prototype, "1", {
        get() {
          return this.length;
        },
        configurable: true,
      });
    }
    if (key === "1") {
      inheritedArrayAccessorSeen.push(value);
    }
    return value;
  });
  console.log(
    "array prototype accessor receiver:",
    inheritedArrayAccessorSeen.join("|"),
    inheritedArrayAccessor.length,
    own(inheritedArrayAccessor, "1"),
    inheritedArrayAccessor[1],
  );
} finally {
  if (inheritedArrayAccessorPrevious) {
    Object.defineProperty(Object.prototype, "1", inheritedArrayAccessorPrevious);
  } else {
    delete (Object.prototype as any)["1"];
  }
}

const arrayNonConfigDelete = JSON.parse("[1,2,3]", function (key, value) {
  if (key === "1") {
    Object.defineProperty(this, "1", {
      value,
      writable: true,
      enumerable: true,
      configurable: false,
    });
    return undefined;
  }
  return value;
});
const arrayNonConfigDeleteDesc = Object.getOwnPropertyDescriptor(arrayNonConfigDelete, "1");
console.log(
  "array nonconfig delete:",
  arrayNonConfigDelete.length,
  own(arrayNonConfigDelete, "1"),
  arrayNonConfigDelete[1],
  Object.keys(arrayNonConfigDelete).join("|"),
  arrayNonConfigDeleteDesc?.configurable,
);

const arrayNonConfigDefine = JSON.parse("[1,2,3]", function (key, value) {
  if (key === "1") {
    Object.defineProperty(this, "1", {
      value,
      writable: false,
      enumerable: true,
      configurable: false,
    });
    return 9;
  }
  return value;
});
const arrayNonConfigDefineDesc = Object.getOwnPropertyDescriptor(arrayNonConfigDefine, "1");
console.log(
  "array nonconfig define:",
  arrayNonConfigDefine[1],
  arrayNonConfigDefineDesc?.writable,
  arrayNonConfigDefineDesc?.configurable,
);

errorLine("array getter abrupt", () =>
  JSON.parse("[1,2,3]", function (key, value) {
    if (key === "0") {
      Object.defineProperty(this, "1", {
        get() {
          throw new Error("boom-array-get");
        },
        enumerable: true,
        configurable: true,
      });
    }
    return value;
  }),
);
