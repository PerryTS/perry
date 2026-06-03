function runStrictCases() {
  "use strict";

  let rhsCount = 0;
  try {
    missingStrictTarget = (rhsCount += 1);
    console.log("strict assignment:", "no throw");
  } catch (error) {
    console.log(
      "strict assignment:",
      error instanceof ReferenceError,
      (error as Error).name,
      rhsCount,
      (globalThis as any).missingStrictTarget === undefined,
    );
  }

  {
    function blockScopedFunction() {
      return "inside-block";
    }
    let blockScopedLet = "let-block";
    console.log("block inside:", blockScopedFunction(), blockScopedLet);
  }

  try {
    console.log("block outside fn:", blockScopedFunction());
  } catch (error) {
    console.log("block outside fn:", error instanceof ReferenceError, (error as Error).name);
  }

  try {
    console.log("block outside let:", blockScopedLet);
  } catch (error) {
    console.log("block outside let:", error instanceof ReferenceError, (error as Error).name);
  }

  switch (1) {
    case 1:
      function switchScopedFunction() {
        return "inside-switch";
      }
      let switchScopedLet = "let-switch";
      var switchScopedVar = "var-switch";
      console.log("switch inside:", switchScopedFunction(), switchScopedLet, switchScopedVar);
      break;
  }

  try {
    console.log("switch outside fn:", switchScopedFunction());
  } catch (error) {
    console.log("switch outside fn:", error instanceof ReferenceError, (error as Error).name);
  }

  try {
    console.log("switch outside let:", switchScopedLet);
  } catch (error) {
    console.log("switch outside let:", error instanceof ReferenceError, (error as Error).name);
  }

  console.log("switch outside var:", switchScopedVar);
}

runStrictCases();
