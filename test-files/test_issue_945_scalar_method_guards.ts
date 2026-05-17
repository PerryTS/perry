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
function positiveScalarMethod(): number {
  const obj = new PositiveScalarMethod(11);
  return obj.getValue();
}
console.log("positive:", positiveScalarMethod());

class SameClassFieldShadow {
  value = 1;
  getValue = () => 22;
  getValue(): number {
    return this.value;
  }
}
function sameClassFieldShadow(): number {
  const obj = new SameClassFieldShadow();
  return obj.getValue();
}
console.log("same-class field shadow:", sameClassFieldShadow());

class ParentFieldShadow {
  getValue = () => 33;
}
class ChildMethodShadow extends ParentFieldShadow {
  value = 3;
  getValue(): number {
    return this.value;
  }
}
function inheritedFieldShadow(): number {
  const obj = new ChildMethodShadow();
  return obj.getValue();
}
console.log("inherited field shadow:", inheritedFieldShadow());

class BaseFieldReturn {
  value = 44;
  getValue(): number {
    return this.value;
  }
}
class ChildInheritedMethod extends BaseFieldReturn {}
function inheritedMethod(): number {
  const obj = new ChildInheritedMethod();
  return obj.getValue();
}
console.log("inherited method:", inheritedMethod());

class ParamMethod {
  value = 50;
  getValue(delta: number): number {
    return this.value + delta;
  }
}
function methodParams(): number {
  const obj = new ParamMethod();
  return obj.getValue(5);
}
console.log("method params:", methodParams());

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
function extraArgMethod(): number {
  const obj = new ExtraArgMethod();
  return (obj as any).getValue(bumpExtraArg());
}
console.log("extra arg:", extraArgMethod());
console.log("extra arg side effect:", extraArgCount);

class NontrivialMethod {
  value = 77;
  getValue(): number {
    const local = this.value;
    return local;
  }
}
function nontrivialMethod(): number {
  const obj = new NontrivialMethod();
  return obj.getValue();
}
console.log("nontrivial body:", nontrivialMethod());

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
function accessorBackedMethod(): number {
  const obj = new AccessorBackedMethod();
  obj.value = 89;
  return obj.getValue();
}
console.log("accessor-backed field:", accessorBackedMethod());
