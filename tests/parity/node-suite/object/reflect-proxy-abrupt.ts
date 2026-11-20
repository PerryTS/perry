function format(value: unknown): string {
  if (value === undefined) {
    return "undefined";
  }
  return JSON.stringify(value);
}

function descSummary(desc: PropertyDescriptor | undefined) {
  if (desc === undefined) {
    return "undefined";
  }
  return {
    value: desc.value,
    writable: desc.writable,
    enumerable: desc.enumerable,
    configurable: desc.configurable,
    hasValue: Object.prototype.hasOwnProperty.call(desc, "value"),
  };
}

function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", "ok", format(fn()));
  } catch (err: any) {
    console.log(label + ":", "throw", err.name);
  }
}

show("gopd primitive target", () => Reflect.getOwnPropertyDescriptor(1 as any, "x"));
show("gopd symbol target", () => Reflect.getOwnPropertyDescriptor(Symbol("target") as any, "x"));
show("gopd key abrupt", () =>
  Reflect.getOwnPropertyDescriptor({}, {
    toString() {
      throw new RangeError("key");
    },
  } as any),
);

const fallbackTarget: any = {};
Object.defineProperty(fallbackTarget, "x", {
  value: 7,
  writable: false,
  enumerable: true,
  configurable: true,
});
show("gopd proxy no trap", () =>
  descSummary(Reflect.getOwnPropertyDescriptor(new Proxy(fallbackTarget, {}), "x")),
);

show("gopd proxy descriptor", () =>
  descSummary(Reflect.getOwnPropertyDescriptor(new Proxy({}, {
    getOwnPropertyDescriptor() {
      return { value: 9, writable: true, enumerable: false, configurable: true };
    },
  }), "x")),
);

show("gopd proxy descriptor proxy object", () =>
  descSummary(Reflect.getOwnPropertyDescriptor(new Proxy({}, {
    getOwnPropertyDescriptor() {
      return new Proxy({ value: 11, writable: true, enumerable: true, configurable: true }, {});
    },
  }), "x")),
);

show("gopd proxy undefined", () =>
  descSummary(Reflect.getOwnPropertyDescriptor(new Proxy({}, {
    getOwnPropertyDescriptor() {
      return undefined;
    },
  }), "x")),
);

show("gopd proxy primitive result", () =>
  Reflect.getOwnPropertyDescriptor(new Proxy({}, {
    getOwnPropertyDescriptor() {
      return 1 as any;
    },
  }), "x"),
);

show("gopd proxy result abrupt", () =>
  Reflect.getOwnPropertyDescriptor(new Proxy({}, {
    getOwnPropertyDescriptor() {
      return {
        value: 1,
        configurable: true,
        get enumerable() {
          throw new RangeError("descriptor");
        },
      };
    },
  }), "x"),
);

show("gopd proxy descriptor proxy abrupt", () =>
  Reflect.getOwnPropertyDescriptor(new Proxy({}, {
    getOwnPropertyDescriptor() {
      return new Proxy({ value: 1, configurable: true }, {
        has(_target, prop) {
          if (prop === "enumerable") {
            throw new RangeError("descriptor proxy");
          }
          return prop in _target;
        },
      });
    },
  }), "x"),
);

show("gopd proxy noncallable trap", () =>
  Reflect.getOwnPropertyDescriptor(new Proxy({}, {
    getOwnPropertyDescriptor: {} as any,
  }), "x"),
);

const revokedDesc = Proxy.revocable({}, {});
revokedDesc.revoke();
show("gopd revoked proxy", () => Reflect.getOwnPropertyDescriptor(revokedDesc.proxy, "x"));

const proto = { marker: "proto" };
const protoTarget = Object.create(proto);
show("getProto proxy no trap", () => Reflect.getPrototypeOf(new Proxy(protoTarget, {})) === proto);
show("getProto proxy object result", () =>
  Reflect.getPrototypeOf(new Proxy({}, {
    getPrototypeOf() {
      return proto;
    },
  })) === proto,
);
show("getProto proxy null result", () =>
  Reflect.getPrototypeOf(new Proxy({}, {
    getPrototypeOf() {
      return null;
    },
  })) === null,
);
show("getProto proxy primitive result", () =>
  Reflect.getPrototypeOf(new Proxy({}, {
    getPrototypeOf() {
      return 1 as any;
    },
  })),
);
show("getProto proxy abrupt", () =>
  Reflect.getPrototypeOf(new Proxy({}, {
    getPrototypeOf() {
      throw new RangeError("prototype");
    },
  })),
);
show("getProto proxy noncallable trap", () =>
  Reflect.getPrototypeOf(new Proxy({}, {
    getPrototypeOf: {} as any,
  })),
);

const revokedProto = Proxy.revocable({}, {});
revokedProto.revoke();
show("getProto revoked proxy", () => Reflect.getPrototypeOf(revokedProto.proxy));
