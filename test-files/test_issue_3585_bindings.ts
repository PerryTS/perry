"use strict";

function fail(label: string): never {
  throw new Error(label);
}

function assertSame(actual: any, expected: any, label: string): void {
  if (actual !== expected) {
    fail(label + ": expected " + expected + ", got " + actual);
  }
}

function assertReferenceError(thunk: () => void, label: string): void {
  try {
    thunk();
  } catch (e) {
    if (e instanceof ReferenceError) {
      return;
    }
    fail(label + ": wrong error");
  }
  fail(label + ": did not throw");
}

function testCatchConstShadow(): void {
  var a = 1;
  try {
    throw "stuff3";
  } catch (a) {
    {
      const a = 3;
      assertSame(a, 3, "inner const shadows catch parameter");
    }
    assertSame(a, "stuff3", "catch parameter survives inner block");
  }
  assertSame(a, 1, "outer var survives catch parameter");
}

function testConstBlockShadowing(): void {
  function fn(a: any): void {
    let b = 1;
    var c = 1;
    const d = 1;
    {
      const a = 2;
      const b = 2;
      const c = 2;
      const d = 2;
      assertSame(a, 2, "block const shadows parameter");
      assertSame(b, 2, "block const shadows let");
      assertSame(c, 2, "block const shadows var");
      assertSame(d, 2, "block const shadows const");
    }
    assertSame(a, 1, "parameter restored after block");
    assertSame(b, 1, "let restored after block");
    assertSame(c, 1, "var restored after block");
    assertSame(d, 1, "const restored after block");
  }
  fn(1);
}

function testFunctionVarHoist(): void {
  var x = 0;
  function f1(): any {
    function f2(): any {
      return x;
    }
    return f2();
    var x = 1;
  }
  assertSame(f1(), undefined, "function-scoped var hoists before nested closure");
  assertSame(x, 0, "outer var not captured when inner var exists");
}

function testFunctionDeclarationConditionCall(): void {
  var x = 0;
  function f1(): any {
    function f2(): any {
      return x;
    }
    return f2();
  }
  if (!(f1() === 0)) {
    fail("function declaration call in condition should preserve scope chain");
  }
}

function testReadBeforeVar(): void {
  if (x !== undefined) {
    fail("read before var should be undefined");
  }
  var x = true;
  var y = false;
  assertSame(x, true, "var x initialized");
  assertSame(y, false, "var y initialized");
}

function testUnresolvableRead(): void {
  assertReferenceError(function () {
    globalThis.z;
    z;
  }, "unresolvable identifier read");
}

function testStrictUnresolvableAssignment(): void {
  assertReferenceError(function () {
    undeclared_lhs = (globalThis.undeclared_rhs = 5);
  }, "strict unresolvable assignment");
  assertSame(globalThis.undeclared_rhs, 5, "assignment RHS evaluated before ReferenceError");
}

function testGlobalThisAndDelete(): void {
  if (delete (0, globalThis) !== true) {
    fail("delete value expression should return true");
  }
  globalThis.nan = NaN;
  if (globalThis.nan === globalThis.nan) {
    fail("globalThis.nan should be NaN after assignment");
  }
  globalThis.nan++;
  if (globalThis.nan === globalThis.nan) {
    fail("globalThis.nan should stay NaN after postfix update");
  }
  ++globalThis.nan;
  if (globalThis.nan === globalThis.nan) {
    fail("globalThis.nan should stay NaN after prefix update");
  }
  globalThis.nan += 1;
  if (globalThis.nan === globalThis.nan) {
    fail("globalThis.nan should stay NaN after compound update");
  }
}

function testDuplicateFunctionDeclaration(): void {
  function f(): number {
    return 1;
  }
  function f(): number {
    return 2;
  }
  assertSame(f(), 2, "duplicate function declaration keeps last body");
}

function testBlockLetRemoved(): void {
  var caught = false;
  try {
    {
      let xx = 18;
      throw 25;
    }
  } catch (e) {
    caught = true;
    assertSame(e, 25, "catch value");
    (function () {
      assertReferenceError(function () {
        xx;
      }, "block let removed before nested function runs");
    })();
  }
  assertSame(caught, true, "outer catch ran");
}

testCatchConstShadow();
testConstBlockShadowing();
testFunctionVarHoist();
testFunctionDeclarationConditionCall();
testReadBeforeVar();
testUnresolvableRead();
testStrictUnresolvableAssignment();
testGlobalThisAndDelete();
testDuplicateFunctionDeclaration();
testBlockLetRemoved();

console.log("issue-3585-bindings: ok");
