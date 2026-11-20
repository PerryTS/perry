// Try/catch/finally, throw, Error
try {
  throw new Error("oops");
} catch (e) {
  console.log(e.message);
}

// Nested try/catch
try {
  try {
    throw new Error("inner");
  } catch (e) {
    console.log("caught: " + e.message);
  }
  console.log("outer ok");
} catch (e) {
  console.log("should not reach");
}

// Finally
let x = 0;
try {
  x = 1;
} finally {
  x = x + 10;
}
console.log(x);

// Bridge exception (JSON.parse)
try {
  JSON.parse("{{bad json");
} catch (e) {
  console.log("json error caught");
}

console.log("done");
