// @ts-nocheck

function show(label, value) {
  console.log(label + ":" + value);
}

function* declared() {
  yield 1;
}

const declaredFirst = declared();
const declaredSecond = declared();
show("declared proto identity", Object.getPrototypeOf(declaredFirst) === declared.prototype);
show(
  "declared calls share proto",
  Object.getPrototypeOf(declaredFirst) === Object.getPrototypeOf(declaredSecond),
);
show(
  "declared proto parent",
  Object.getPrototypeOf(Object.getPrototypeOf(declaredFirst)) ===
    Object.getPrototypeOf(declared).prototype,
);

const overrideProto = { marker: "custom" };
declared.prototype = overrideProto;
show("declared override identity", Object.getPrototypeOf(declared()) === overrideProto);

const expr = function* () {
  yield 2;
};

const exprFirst = expr();
const exprSecond = expr();
show("expr proto identity", Object.getPrototypeOf(exprFirst) === expr.prototype);
show("expr calls share proto", Object.getPrototypeOf(exprFirst) === Object.getPrototypeOf(exprSecond));

const alias = expr;
show("alias proto identity", Object.getPrototypeOf(alias()) === expr.prototype);

async function* asyncDeclared() {
  yield 3;
}

show("async declared proto identity", Object.getPrototypeOf(asyncDeclared()) === asyncDeclared.prototype);
