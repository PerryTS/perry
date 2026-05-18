// Regression coverage for issue #945's scalar field-return method fast path.
// The optimization is intentionally narrow: it may only fire when the receiver
// is a live exact `new C(...)` local and method lookup is statically stable.
// Dynamic prototype mutation is outside this parity fixture because Perry's
// runtime does not yet implement those Node dispatch semantics byte-for-byte.

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

class VirtualBaseMethod {
  value = 12;
  getValue(): number {
    return this.value;
  }
}
class VirtualChildOverride extends VirtualBaseMethod {
  value = 13;
  getValue(): number {
    return 90;
  }
}
function readVirtualBase(x: VirtualBaseMethod): number {
  return x.getValue();
}
function virtualDispatchOverride(): number {
  const obj = new VirtualChildOverride();
  return readVirtualBase(obj);
}
console.log("virtual dispatch override:", virtualDispatchOverride());

class OwnMethodWrite {
  value = 14;
  getValue(): number {
    return this.value;
  }
}
function ownMethodWrite(): number {
  const obj = new OwnMethodWrite();
  (obj as any).getValue = () => 99;
  return obj.getValue();
}
console.log("own method write:", ownMethodWrite());

class DefinePropertyMethodWrite {
  value = 15;
  getValue(): number {
    return this.value;
  }
}
function definePropertyMethodWrite(): number {
  const obj = new DefinePropertyMethodWrite();
  Object.defineProperty(obj, "getValue", { value: () => 100 });
  return obj.getValue();
}
console.log("defineProperty method write:", definePropertyMethodWrite());

class ComputedMethodWrite {
  value = 16;
  getValue(): number {
    return this.value;
  }
}
function computedMethodWrite(): number {
  const obj = new ComputedMethodWrite();
  (obj as any)["getValue"] = () => 101;
  return obj.getValue();
}
console.log("computed method write:", computedMethodWrite());

class AliasMethodWrite {
  value = 17;
  getValue(): number {
    return this.value;
  }
}
function aliasMethodWrite(): number {
  const obj = new AliasMethodWrite();
  const alias = obj as any;
  alias.getValue = () => 102;
  return obj.getValue();
}
console.log("alias method write:", aliasMethodWrite());

class EscapedMethodWrite {
  value = 18;
  getValue(): number {
    return this.value;
  }
}
function mutateEscapedMethod(target: any): void {
  target.getValue = () => 103;
}
function escapedMethodWrite(): number {
  const obj = new EscapedMethodWrite();
  mutateEscapedMethod(obj);
  return obj.getValue();
}
console.log("escaped method write:", escapedMethodWrite());

class ConstructorOwnMethodWrite {
  value = 19;
  constructor() {
    (this as any).getValue = () => 119;
  }
  getValue(): number {
    return this.value;
  }
}
function constructorOwnMethodWrite(): number {
  const obj = new ConstructorOwnMethodWrite();
  return obj.getValue();
}
console.log("constructor own method write:", constructorOwnMethodWrite());

class ConstructorDefinePropertyMethod {
  value = 20;
  constructor() {
    Object.defineProperty(this, "getValue", {
      value: function () {
        return 120;
      },
    });
  }
  getValue(): number {
    return this.value;
  }
}
function constructorDefinePropertyMethod(): number {
  const obj = new ConstructorDefinePropertyMethod();
  return obj.getValue();
}
console.log(
  "constructor defineProperty method:",
  constructorDefinePropertyMethod(),
);

class ConstructorComputedMethodWrite {
  value = 21;
  constructor() {
    const key = "getValue";
    (this as any)[key] = () => 121;
  }
  getValue(): number {
    return this.value;
  }
}
function constructorComputedMethodWrite(): number {
  const obj = new ConstructorComputedMethodWrite();
  return obj.getValue();
}
console.log(
  "constructor computed method write:",
  constructorComputedMethodWrite(),
);

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

let inlineArgCount = 0;
function bumpInlineArg(): number {
  inlineArgCount += 1;
  return 7;
}
class TwiceArgMethod {
  twice(value: number): number {
    return value + value;
  }
}
function sideEffectArgOnce(): number {
  const obj = new TwiceArgMethod();
  return obj.twice(bumpInlineArg());
}
console.log("side-effect arg once:", sideEffectArgOnce());
console.log("side-effect arg count:", inlineArgCount);

let argOrderTrace = "";
function recordArgOrder(value: number): number {
  argOrderTrace += `${value}`;
  return value;
}
class ArgOrderMethod {
  combine(left: number, right: number): number {
    return left * 10 + right;
  }
}
function argumentLeftToRight(): string {
  const obj = new ArgOrderMethod();
  const result = obj.combine(recordArgOrder(1), recordArgOrder(2));
  return `${result}:${argOrderTrace}`;
}
console.log("argument left-to-right:", argumentLeftToRight());

function missingArgFunction(value?: number): number {
  return value === undefined ? 201 : value;
}
function missingArgUndefined(): number {
  const value = 777;
  return missingArgFunction();
}
console.log("missing arg undefined:", missingArgUndefined());

class MissingArgMethod {
  read(value?: number): number {
    return value === undefined ? 202 : value;
  }
}
function missingMethodArgUndefined(): number {
  const obj = new MissingArgMethod();
  const value = 778;
  return obj.read();
}
console.log("missing method arg undefined:", missingMethodArgUndefined());

class ReflectMethodWrite {
  value = 29;
  getValue(): number {
    return this.value;
  }
}
function reflectMethodWrite(): number {
  const obj = new ReflectMethodWrite();
  Reflect.defineProperty(obj, "getValue", { value: () => 128 });
  return obj.getValue();
}
console.log("Reflect.defineProperty method write:", reflectMethodWrite());

class ReassignedReceiverMethod {
  value = 31;
  getValue(): number {
    return this.value;
  }
}
function reassignedReceiverMethod(): number {
  let obj = new ReassignedReceiverMethod();
  obj = new ReassignedReceiverMethod();
  (obj as any).getValue = () => 130;
  return obj.getValue();
}
console.log("reassigned receiver:", reassignedReceiverMethod());

class BranchAmbiguousReceiverMethod {
  value = 32;
  getValue(): number {
    return this.value;
  }
}
function branchAmbiguousReceiverMethod(flag: boolean): number {
  let obj: BranchAmbiguousReceiverMethod;
  if (flag) {
    obj = new BranchAmbiguousReceiverMethod();
  } else {
    obj = new BranchAmbiguousReceiverMethod();
    (obj as any).getValue = () => 131;
  }
  return obj.getValue();
}
console.log("branch ambiguity:", branchAmbiguousReceiverMethod(false));

class LoopMutationReceiverMethod {
  value = 33;
  getValue(): number {
    return this.value;
  }
}
function loopMutationReceiverMethod(): number {
  const obj = new LoopMutationReceiverMethod();
  for (let i = 0; i < 1; i += 1) {
    (obj as any).getValue = () => 132;
  }
  return obj.getValue();
}
console.log("loop mutation receiver:", loopMutationReceiverMethod());

class ClosureCapturedReceiverMethod {
  value = 34;
  getValue(): number {
    return this.value;
  }
}
function closureCapturedReceiverMethod(): number {
  const obj = new ClosureCapturedReceiverMethod();
  const mutate = () => {
    (obj as any).getValue = () => 133;
  };
  mutate();
  return obj.getValue();
}
console.log("closure captured receiver:", closureCapturedReceiverMethod());

class SameExprScalarMethod {
  value = 20;
  getValue(): number {
    return this.value;
  }
}
function sameExprOwnWrite(): number {
  const obj = new SameExprScalarMethod();
  return ((obj as any).getValue = () => 104, obj.getValue());
}
console.log("same-expr own write:", sameExprOwnWrite());

function sameExprDefineProperty(): number {
  const obj = new SameExprScalarMethod();
  return (
    Object.defineProperty(obj, "getValue", { value: () => 105 }),
    obj.getValue()
  );
}
console.log("same-expr defineProperty:", sameExprDefineProperty());

function sameExprComputedWrite(): number {
  const obj = new SameExprScalarMethod();
  const key = "getValue";
  return ((obj as any)[key] = () => 106, obj.getValue());
}
console.log("same-expr computed write:", sameExprComputedWrite());

function mutateSameExpr(target: any, value: number): number {
  target.getValue = () => value;
  return 0;
}
function sameExprCallEscapeBinary(): number {
  const obj = new SameExprScalarMethod();
  return mutateSameExpr(obj, 107) + obj.getValue();
}
console.log("same-expr call escape binary:", sameExprCallEscapeBinary());

function mutateSameExprTruthy(target: any, value: number): boolean {
  target.getValue = () => value;
  return true;
}
function sameExprLogicalMutation(): number {
  const obj = new SameExprScalarMethod();
  return mutateSameExprTruthy(obj, 108) && obj.getValue();
}
console.log("same-expr logical mutation:", sameExprLogicalMutation());

function sameExprConditionalMutation(): number {
  const obj = new SameExprScalarMethod();
  return mutateSameExprTruthy(obj, 109) ? obj.getValue() : -1;
}
console.log("same-expr conditional mutation:", sameExprConditionalMutation());

function identityNumber(value: number): number {
  return value;
}
function sameExprHoistedArgMutation(): number {
  const obj = new SameExprScalarMethod();
  return identityNumber(mutateSameExpr(obj, 110)) + obj.getValue();
}
console.log("same-expr hoisted arg mutation:", sameExprHoistedArgMutation());

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
