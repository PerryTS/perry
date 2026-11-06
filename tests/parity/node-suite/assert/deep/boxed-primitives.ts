import assert from "node:assert";

try {
  assert.deepStrictEqual(new Number(1), new Number(1));
  console.log("number objects equal: pass");
} catch (err) {
  console.log("number objects equal: fail", (err as Error).name);
}

try {
  assert.deepStrictEqual(new Number(1), new Number(2));
  console.log("number objects different: pass-unexpected");
} catch (err) {
  console.log("number objects different:", (err as Error).name, (err as { code?: string }).code);
}

try {
  assert.deepStrictEqual(new String("x"), new String("x"));
  console.log("string objects equal: pass");
} catch (err) {
  console.log("string objects equal: fail", (err as Error).name);
}

try {
  assert.deepStrictEqual(new String("x"), new String("y"));
  console.log("string objects different: pass-unexpected");
} catch (err) {
  console.log("string objects different:", (err as Error).name, (err as { code?: string }).code);
}

try {
  assert.deepStrictEqual(new Boolean(false), new Boolean(true));
  console.log("boolean objects different: pass-unexpected");
} catch (err) {
  console.log("boolean objects different:", (err as Error).name, (err as { code?: string }).code);
}
