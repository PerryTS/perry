const log: string[] = [];
const receiver: any = { marker: "original" };

const A = {
  set x(value: number) {
    log.push(`A:${value}:${this === receiver}:${receiver.marker}`);
    receiver.seen = `A${value}`;
  },
};
const B = {
  set x(value: number) {
    log.push(`B:${value}:${this === receiver}:${receiver.marker}`);
    receiver.seen = `B${value}`;
  },
};
const C = {
  set x(value: number) {
    log.push(`C:${value}:${this === receiver}:${receiver.marker}`);
    receiver.seen = `C${value}`;
  },
};

const makeKey = (throwOnCoerce = false) => {
  log.push(throwOnCoerce ? "key-expression-throw" : "key-expression");
  return {
    [Symbol.toPrimitive]() {
      log.push(throwOnCoerce ? "throw-key" : "key");
      receiver.marker = throwOnCoerce ? "throw-coerced" : "coerced";
      Object.setPrototypeOf(Home.prototype, C);
      if (throwOnCoerce) throw new Error("key conversion");
      return "x";
    },
  };
};

class Home {
  write(value: number, throwOnCoerce = false) {
    super[makeKey(throwOnCoerce)] =
      (log.push(`rhs:${value}`), Object.setPrototypeOf(Home.prototype, B), value);
  }
}

Object.setPrototypeOf(Home.prototype, A);
Home.prototype.write.call(receiver, 1);
console.log("instance", JSON.stringify(log), receiver.seen);

log.length = 0;
try {
  Home.prototype.write.call(receiver, 2, true);
} catch (error: any) {
  log.push(`caught:${error.message}`);
}
console.log("throw", JSON.stringify(log), receiver.seen);

const staticLog: string[] = [];
const staticKey = () => {
  staticLog.push("key-expression");
  return {
    [Symbol.toPrimitive]() {
      staticLog.push("key");
      return "x";
    },
  };
};
class StaticBase {
  static set x(value: number) {
    staticLog.push(`static:${value}:${this === StaticHome}`);
    (this as any).seen = value;
  }
}
class StaticHome extends StaticBase {
  static write(value: number) {
    super[staticKey()] = (staticLog.push(`rhs:${value}`), value);
  }
}
StaticHome.write(3);
console.log("static", JSON.stringify(staticLog), StaticHome.seen);
