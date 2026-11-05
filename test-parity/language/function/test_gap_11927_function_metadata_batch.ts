function declared(value: number) {
  return value + 1;
}

const expression = function visibleExpression(value: number) {
  return value * 2;
};

function throwsFromDeclared() {
  throw new Error("metadata-batch");
}

console.log(declared.name, declared(2));
console.log(expression.name, expression(3));
console.log(declared.toString().includes("return value + 1"));
console.log(expression.toString().includes("function visibleExpression"));

try {
  throwsFromDeclared();
} catch (error) {
  const stack = (error as Error).stack || "";
  console.log(stack.split("\n").some((line) => line.includes("throwsFromDeclared")));
}
