// Regression coverage for issue #945's scalar field-return method fast path.
// The optimization is intentionally narrow: it may only fire when
// receiver.method() is a zero-arg prototype method whose whole body is
// `return this.field`, with no own-property or accessor shadowing.

class PositiveScalarMethod {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
  getValue(): number {
    return this.value;
  }
}
console.log("positive:", new PositiveScalarMethod(11).getValue());

class SameClassFieldShadow {
  value = 1;
  getValue = () => 22;
  getValue(): number {
    return this.value;
  }
}
console.log("same-class field shadow:", new SameClassFieldShadow().getValue());

class ParentFieldShadow {
  getValue = () => 33;
}
class ChildMethodShadow extends ParentFieldShadow {
  value = 3;
  getValue(): number {
    return this.value;
  }
}
console.log("inherited field shadow:", new ChildMethodShadow().getValue());

class BaseFieldReturn {
  value = 44;
  getValue(): number {
    return this.value;
  }
}
class ChildInheritedMethod extends BaseFieldReturn {}
console.log("inherited method:", new ChildInheritedMethod().getValue());

class ParamMethod {
  value = 50;
  getValue(delta: number): number {
    return this.value + delta;
  }
}
console.log("method params:", new ParamMethod().getValue(5));

let extraArgCount = 0;
function bumpExtraArg(): number {
  extraArgCount += 1;
  return 99;
}
class ExtraArgMethod {
  value = 66;
  getValue(): number {
    return this.value;
  }
}
console.log("extra arg:", (new ExtraArgMethod() as any).getValue(bumpExtraArg()));
console.log("extra arg side effect:", extraArgCount);

class NontrivialMethod {
  value = 77;
  getValue(): number {
    const local = this.value;
    return local;
  }
}
console.log("nontrivial body:", new NontrivialMethod().getValue());

class AccessorBackedMethod {
  private _value = 88;
  get value(): number {
    return this._value;
  }
  set value(next: number) {
    this._value = next;
  }
  getValue(): number {
    return this.value;
  }
}
const accessorBacked = new AccessorBackedMethod();
accessorBacked.value = 89;
console.log("accessor-backed field:", accessorBacked.getValue());
