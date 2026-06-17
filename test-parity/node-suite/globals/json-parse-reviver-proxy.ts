function shape(label: string, fn: () => unknown): void {
  try {
    fn();
    console.log(label + ": ok");
  } catch (err) {
    const e = err as Error;
    console.log(label + ":", e.name + ":" + String(e.message).split("\n")[0]);
  }
}

shape("define object proxy", () => {
  const proxy = new Proxy({ 0: null }, {
    defineProperty() {
      throw new Error("define-object");
    },
  });
  JSON.parse('["first",null]', function (_key, value) {
    if (value === "first") {
      this[1] = proxy;
    }
    return value;
  });
});

shape("define array proxy", () => {
  const proxy = new Proxy([null], {
    defineProperty() {
      throw new Error("define-array");
    },
  });
  JSON.parse('["first",null]', function (_key, value) {
    if (value === "first") {
      this[1] = proxy;
    }
    return value;
  });
});

shape("delete object proxy", () => {
  const proxy = new Proxy({ a: 1 }, {
    deleteProperty() {
      throw new Error("delete-object");
    },
  });
  JSON.parse("[0,0]", function (key) {
    if (key !== "") {
      this[1] = proxy;
    }
  });
});

shape("delete array proxy", () => {
  const proxy = new Proxy([0], {
    deleteProperty() {
      throw new Error("delete-array");
    },
  });
  JSON.parse("[0,0]", function (key) {
    if (key !== "") {
      this[1] = proxy;
    }
  });
});

shape("ownKeys object proxy", () => {
  const proxy = new Proxy({}, {
    ownKeys() {
      throw new Error("own-keys");
    },
  });
  JSON.parse("[0,0]", function (key) {
    if (key !== "") {
      this[1] = proxy;
    }
  });
});

shape("length get proxy", () => {
  const proxy = new Proxy([], {
    get(target, key) {
      if (key === "length") {
        throw new Error("length-get");
      }
      return Reflect.get(target, key);
    },
  });
  JSON.parse("[0,0]", function (key) {
    if (key !== "") {
      this[1] = proxy;
    }
  });
});

shape("length coerce proxy", () => {
  const proxy = new Proxy([], {
    get(target, key) {
      if (key === "length") {
        return {
          valueOf() {
            throw new Error("length-coerce");
          },
        };
      }
      return Reflect.get(target, key);
    },
  });
  JSON.parse("[0,0]", function (key) {
    if (key !== "") {
      this[1] = proxy;
    }
  });
});

function proxyVisitsOther(injected: unknown): boolean {
  let visited = false;
  JSON.parse("[null,null]", function (key, value) {
    if (key === "other") {
      visited = true;
    }
    this[1] = injected;
    return value;
  });
  return visited;
}

const objectProxy = new Proxy({ length: 0, other: 0 }, {});
const arrayProxy = new Proxy([], {});
(arrayProxy as any).other = 0;
const arrayProxyProxy = new Proxy(arrayProxy, {});

console.log("proxy object visits other:", proxyVisitsOther(objectProxy));
console.log("proxy array visits other:", proxyVisitsOther(arrayProxy));
console.log("proxy array proxy visits other:", proxyVisitsOther(arrayProxyProxy));

const handle = Proxy.revocable([], {});
let revokedReturns = 0;
handle.revoke();
shape("revoked proxy", () => {
  JSON.parse("[null,null]", function (_key, value) {
    this[1] = handle.proxy;
    revokedReturns += 1;
    return value;
  });
});
console.log("revoked returns:", revokedReturns);
