import { collect } from "perry/gc";

let changed = 1;
const retained = { text: "root survives" };
try {
  changed = 2;
  collect();
  throw new Error("first");
} catch (error: any) {
  console.log(changed, error.message, retained.text);
} finally {
  console.log("outer finally");
}

try {
  try {
    throw new Error("original");
  } catch (error) {
    collect();
    throw error;
  } finally {
    console.log("rethrow finally");
  }
} catch (error: any) {
  console.log(error.message);
}

// Throws created inside the runtime must reach a generated handler.
try {
  JSON.parse("{");
} catch (error: any) {
  console.log(error instanceof SyntaxError);
}

// A runtime callback boundary must catch, settle the next promise and
// continue draining jobs without leaving the GC root stack corrupted.
Promise.resolve(1).then(() => {
  collect();
  throw new Error("callback");
}).catch((error: any) => {
  console.log(error.message);
  collect();
  console.log(retained.text);
});
