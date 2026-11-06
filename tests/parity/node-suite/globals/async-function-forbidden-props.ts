// @ts-nocheck

const missing = Symbol("missing");

function show(label, value) {
  console.log(label + ":" + String(value));
}

async function asyncDecl() {
  show("async own caller", asyncDecl.hasOwnProperty("caller"));
  show("async own arguments", asyncDecl.hasOwnProperty("arguments"));
}

function ownValueProbe() {
  function inner() {
    return inner.hasOwnProperty("caller") ? inner.caller : missing;
  }

  const descriptor = Object.getOwnPropertyDescriptor(inner, "caller");
  if (descriptor && descriptor.configurable) {
    Object.defineProperty(inner, "caller", { value: 1 });
  }

  const result = inner();
  show("own value caller", result === missing ? "missing" : result);
}

function ownGetterProbe() {
  function inner() {
    return inner.hasOwnProperty("caller") ? inner.caller : missing;
  }

  const descriptor = Object.getOwnPropertyDescriptor(inner, "caller");
  if (descriptor && descriptor.configurable) {
    Object.defineProperty(inner, "caller", {
      get() {
        return 2;
      },
    });
  }

  const result = inner();
  show("own getter caller", result === missing ? "missing" : result);
}

function userDefinedCallerProbe() {
  function inner() {}
  Object.defineProperty(inner, "caller", { value: 3, configurable: true });
  show("defined caller own", inner.hasOwnProperty("caller"));
  show("defined caller value", inner.caller);
}

function userDefinedCallerAccessorProbe() {
  function inner() {}
  Object.defineProperty(inner, "caller", {
    get() {
      return 4;
    },
    enumerable: true,
    configurable: true,
  });
  show("defined caller getter own", inner.hasOwnProperty("caller"));
  show("defined caller getter value", inner.caller);
  show("defined caller getter names", Object.getOwnPropertyNames(inner).includes("caller"));
  show("defined caller getter keys", Object.keys(inner).join(","));
  show("defined caller getter values", Object.values(inner).join(","));
}

async function main() {
  await asyncDecl();
  ownValueProbe();
  ownGetterProbe();
  userDefinedCallerProbe();
  userDefinedCallerAccessorProbe();
}

main();
