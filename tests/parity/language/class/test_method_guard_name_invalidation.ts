// Direct-method guards are invalidated by method name. Unrelated prototype
// writes must leave a hot guard usable, while writes for the guarded name must
// fall back and observe the replacement across an inheritance chain.

class MethodGuardBase {
  value: number;

  constructor(value: number) {
    this.value = value;
  }

  hot(): string {
    return "base:" + this.value;
  }
}

class MethodGuardChild extends MethodGuardBase {}

class MethodGuardOther {
  cold(): string {
    return "cold";
  }
}

function callHot(receiver: MethodGuardBase): string {
  return receiver.hot();
}

const receiver: MethodGuardBase = new MethodGuardChild(7);
console.log(callHot(receiver));

class MethodGuardDelete {
  gone(): string {
    return "present";
  }
}

function callGone(receiver: MethodGuardDelete): string {
  return receiver.gone();
}

const deleted = new MethodGuardDelete();
console.log(callGone(deleted));
delete (MethodGuardDelete.prototype as any).gone;
try {
  console.log(callGone(deleted));
} catch (_error) {
  console.log("deleted");
}

(MethodGuardOther.prototype as any).cold = function (): string {
  return "patched-cold";
};
console.log(callHot(receiver));

// A same-name write on any class conservatively retires the hash slot. It
// must not change this receiver's answer, but subsequent direct guards may no
// longer assume that `hot` is untouched.
(MethodGuardOther.prototype as any).hot = function (): string {
  return "other-hot";
};
console.log(callHot(receiver));

(MethodGuardBase.prototype as any).hot = function (this: MethodGuardBase): string {
  return "patched:" + this.value;
};
console.log(callHot(receiver));
