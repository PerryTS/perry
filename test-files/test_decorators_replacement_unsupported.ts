// Canary for the maintainer's PR #754 ask: class decorators that return a
// replacement class are now rejected with a runtime TypeError at decorator
// application time. Silent failure (Perry running the decorator for side
// effects and discarding the return) is what this test guards against.
//
// Decorator-init lives in module.init in Perry's lowering, so the throw
// fires before any user code runs — that's why the console.log below is
// unreachable. The expected-output file captures the TypeError header
// Perry prints for an uncaught throw at module init.

function ReplaceWithOther() {
  return function (_target: any) {
    return class Other {
      static marker = "replacement";
    };
  };
}

@ReplaceWithOther()
class Original {
  static marker = "original";
}

// Unreachable: the throw above stops module init.
console.log("unreachable", Original.marker);
