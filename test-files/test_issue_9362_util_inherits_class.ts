// #9362: util.inherits must support all function/class constructor pairings.
import util from "node:util";

function FunctionFunctionBase() {}
(FunctionFunctionBase.prototype as any).baseKind = function () {
  return "function/function";
};
function FunctionFunctionDerived() {}
util.inherits(FunctionFunctionDerived, FunctionFunctionBase);
const ffSuper = (FunctionFunctionDerived as any).super_;
const ffDescriptor = Object.getOwnPropertyDescriptor(FunctionFunctionDerived, "super_")!;
const ffInstance = new FunctionFunctionDerived();
console.log(
  "function/function super:",
  typeof ffSuper,
  ffSuper === FunctionFunctionBase,
  ffDescriptor.writable,
  ffDescriptor.enumerable,
  ffDescriptor.configurable,
);
console.log(
  "function/function chain:",
  Object.getPrototypeOf(FunctionFunctionDerived.prototype) ===
    FunctionFunctionBase.prototype,
  ffInstance instanceof FunctionFunctionDerived,
  ffInstance instanceof FunctionFunctionBase,
  (ffInstance as any).baseKind(),
);

class FunctionClassBase {
  baseKind() {
    return "function/class";
  }
}
function FunctionClassDerived() {}
util.inherits(FunctionClassDerived, FunctionClassBase);
const fcSuper = (FunctionClassDerived as any).super_;
const fcDescriptor = Object.getOwnPropertyDescriptor(
  FunctionClassDerived,
  "super_",
)!;
const fcInstance = new FunctionClassDerived();
console.log(
  "function/class super:",
  typeof fcSuper,
  fcSuper === FunctionClassBase,
  fcDescriptor.writable,
  fcDescriptor.enumerable,
  fcDescriptor.configurable,
);
console.log(
  "function/class chain:",
  Object.getPrototypeOf(FunctionClassDerived.prototype) ===
    FunctionClassBase.prototype,
  fcInstance instanceof FunctionClassDerived,
  fcInstance instanceof FunctionClassBase,
  (fcInstance as any).baseKind(),
);

function ClassFunctionBase() {}
(ClassFunctionBase.prototype as any).baseKind = function () {
  return "class/function";
};
class ClassFunctionDerived {}
util.inherits(ClassFunctionDerived, ClassFunctionBase);
const cfSuper = (ClassFunctionDerived as any).super_;
const cfDescriptor = Object.getOwnPropertyDescriptor(
  ClassFunctionDerived,
  "super_",
)!;
const cfInstance = new ClassFunctionDerived();
console.log(
  "class/function super:",
  typeof cfSuper,
  cfSuper === ClassFunctionBase,
  cfDescriptor.writable,
  cfDescriptor.enumerable,
  cfDescriptor.configurable,
);
console.log(
  "class/function chain:",
  Object.getPrototypeOf(ClassFunctionDerived.prototype) ===
    ClassFunctionBase.prototype,
  cfInstance instanceof ClassFunctionDerived,
  cfInstance instanceof ClassFunctionBase,
  (cfInstance as any).baseKind(),
);

class ClassClassBase {
  baseKind() {
    return "class/class";
  }
}
class ClassClassDerived {}
util.inherits(ClassClassDerived, ClassClassBase);
const ccSuper = (ClassClassDerived as any).super_;
const ccDescriptor = Object.getOwnPropertyDescriptor(
  ClassClassDerived,
  "super_",
)!;
const ccInstance = new ClassClassDerived();
console.log(
  "class/class super:",
  typeof ccSuper,
  ccSuper === ClassClassBase,
  ccDescriptor.writable,
  ccDescriptor.enumerable,
  ccDescriptor.configurable,
);
console.log(
  "class/class chain:",
  Object.getPrototypeOf(ClassClassDerived.prototype) ===
    ClassClassBase.prototype,
  ccInstance instanceof ClassClassDerived,
  ccInstance instanceof ClassClassBase,
  ccInstance.baseKind(),
);
