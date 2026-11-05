// Each Node-API function returns the object it built, whether or not it built
// it inside a handle scope of its own that closed before the return.
const addon = require("fixture-scope-return");
for (const name of ["openNamed", "openScoped", "openNested", "openEscaped"]) {
  const result = addon[name]();
  console.log(name, typeof result, JSON.stringify(result));
}
