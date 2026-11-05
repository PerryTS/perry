// Regression: flatMap must flatten only Arrays, never arbitrary POINTER_TAG values.

const plainObject = { x: 7 };
const objectResult = [1].flatMap(() => plainObject as any);
console.log("object:", objectResult.length, objectResult[0] === plainObject, objectResult[0].x);

const closure = () => 42;
const functionResult = [1].flatMap(() => closure as any);
console.log("function:", functionResult.length, functionResult[0] === closure, functionResult[0]());

console.log("array:", [1, 2].flatMap((x) => [x, x * 10]).join(","));

const sparse = [1, , 3] as number[];
console.log("sparse:", sparse.flatMap((x) => [, x]).join(","));

console.log("nested:", JSON.stringify([1].flatMap((x) => [[x, x + 1]])));

const proxiedArray = new Proxy([8, 9], {});
console.log("proxy-array:", [1].flatMap(() => proxiedArray).join(","));

const nestedProxiedArray = new Proxy(proxiedArray, {});
console.log("nested-proxy-array:", [1].flatMap(() => nestedProxiedArray).join(","));
