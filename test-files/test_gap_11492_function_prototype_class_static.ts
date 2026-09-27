// #11492: a user member installed on Function.prototype must be reachable
// through a class constructor (read, call, `in`), just as through a plain
// function — a constructor's [[Prototype]] chain ends at %Function.prototype%.
// Own and inherited statics keep shadowing it.

const t = (label: string, fn: () => unknown) => {
  try {
    console.log(label, fn());
  } catch (e) {
    console.log(label, "THROW", (e as Error).constructor.name);
  }
};

const FP = Function.prototype as any;
FP.myHelper = function (this: any) {
  return "fp:" + typeof this + ":" + (this && this.name);
};
FP.helper = function () {
  return "fp-helper";
};
FP.extra = function (this: any, x: number, y: number) {
  return "fp-extra:" + this.name + ":" + x + ":" + y;
};

class K {
  static helper() {
    return "K.helper";
  }
  static withArg(x: number) {
    return x * 2;
  }
  static field = 7;
}
class Sub extends K {
  static own() {
    return "Sub.own";
  }
}
class Plain {}
class OnlyField {
  static n = 1;
}
const Expr = class Named {
  static helper() {
    return "Expr.helper";
  }
};
function f() {
  return 1;
}

// Own / inherited statics still win over Function.prototype.
t("K.helper", () => K.helper());
t("K.withArg", () => K.withArg(21));
t("K.field", () => K.field);
t("Sub.helper", () => Sub.helper());
t("Sub.own", () => Sub.own());
t("Expr.helper", () => Expr.helper());
t("OnlyField.helper", () => (OnlyField as any).helper());
t("Plain.helper", () => (Plain as any).helper());

// Direct calls through the class binding, with `this` = the class.
t("K.myHelper", () => (K as any).myHelper());
t("Sub.myHelper", () => (Sub as any).myHelper());
t("Plain.myHelper", () => (Plain as any).myHelper());
t("Expr.myHelper", () => (Expr as any).myHelper());
t("f.myHelper", () => (f as any).myHelper());
t("K.extra", () => (K as any).extra(1, 2));
t("Sub.extra", () => (Sub as any).extra(3, 4));

// Value reads and dynamic calls.
const k: any = K;
const key = "my" + "Helper";
t("typeof K.myHelper", () => typeof (K as any).myHelper);
t("K.myHelper === FP.myHelper", () => (K as any).myHelper === FP.myHelper);
t("k.helper", () => k.helper());
t("k.myHelper", () => k.myHelper());
t("K[key]()", () => (K as any)[key]());
t("detached call", () => {
  const m = (Sub as any).extra;
  return m.call(Plain, 5, 6);
});
t("K.helper.call", () => K.helper.call(null));
t("K.call", () => typeof K.call);

// [[HasProperty]] agrees with [[Get]]; not an own property.
t("'myHelper' in K", () => "myHelper" in K);
t("'myHelper' in Sub", () => "myHelper" in Sub);
t("'myHelper' in K.prototype", () => "myHelper" in K.prototype);
t("hasOwn K myHelper", () => Object.prototype.hasOwnProperty.call(K, "myHelper"));
t("'missing' in K", () => "missing" in K);

// An accessor on Function.prototype runs with the class as receiver.
Object.defineProperty(Function.prototype, "tag", {
  get(this: any) {
    return "tag:" + this.name;
  },
  configurable: true,
});
t("K.tag", () => (K as any).tag);
t("Sub.tag", () => (Sub as any).tag);
t("'tag' in Plain", () => "tag" in Plain);

// Instances do not see Function.prototype members.
t("new K().myHelper", () => typeof (new K() as any).myHelper);

// Deleting the member removes it again.
delete FP.myHelper;
t("after delete typeof", () => typeof (K as any).myHelper);
t("after delete in", () => "myHelper" in K);
t("after delete call", () => (K as any).myHelper());
t("after delete K.helper", () => K.helper());
