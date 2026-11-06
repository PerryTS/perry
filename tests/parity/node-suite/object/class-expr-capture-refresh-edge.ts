// #6654: capture refreshes must stay attached to the evaluated class object
// across parameter defaults, repeated factory evaluation, and top-level blocks.

function defaultInBlock(
  x,
  C = class {
    get() {
      return x;
    }
  },
) {
  x = "body";
  return C;
}
console.log("param-block:", new (defaultInBlock("initial"))().get());

const defaultInArrow = (
  x,
  C = class {
    get() {
      return x;
    }
  },
  update = (x = "later-param"),
) => C;
console.log("param-arrow:", new (defaultInArrow("initial"))().get());

const updateInArrowExpression = (x, C) => (
  (C = class {
    get() {
      return x;
    }
  }),
  (x = "expression"),
  C
);
console.log(
  "expression-arrow:",
  new (updateInArrowExpression("initial"))().get(),
);

function makeAssignedAfter(tag) {
  const C = class {
    get() {
      return value;
    }
  };
  const value = tag;
  return C;
}
const A = makeAssignedAfter("a");
const B = makeAssignedAfter("b");
console.log("multi-eval:", new A().get(), new B().get());

function makeStaticAssignedAfter(tag) {
  const C = class {
    static get() {
      return value;
    }
  };
  const value = tag;
  return C;
}
const StaticA = makeStaticAssignedAfter("sa");
const StaticB = makeStaticAssignedAfter("sb");
console.log("multi-static:", StaticA.get(), StaticB.get());

const escaped = {};
{
  let value = "before";
  escaped.K = class {
    get() {
      return value;
    }
  };
  value = "after";
}
console.log("top-block:", new escaped.K().get());
