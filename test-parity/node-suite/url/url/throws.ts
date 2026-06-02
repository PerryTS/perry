try {
  new URL("not a url");
  console.log("invalid: no throw");
} catch (e) {
  console.log("invalid: threw TypeError:", e instanceof TypeError);
}

try {
  new URL("/relative-only");
  console.log("relative no base: no throw");
} catch (e) {
  console.log("relative no base: threw TypeError:", e instanceof TypeError);
}

try {
  new URL("/p", "not a base");
  console.log("invalid base: no throw");
} catch (e) {
  console.log("invalid base: threw TypeError:", e instanceof TypeError);
}
