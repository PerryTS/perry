import process from "node:process";

const env = process.env;
const original = "Perry_Case_Insensitive_6622";
const upper = original.toUpperCase();
const lower = original.toLowerCase();

if (process.platform === "win32") {
  delete env[lower];
  env[original] = "first";
  console.log("case get:", env[upper] === "first");
  console.log("case has:", upper in env, Object.hasOwn(env, lower));
  console.log(
    "one enumerated spelling:",
    Object.keys(env).filter((key) => key.toUpperCase() === upper).length === 1,
  );

  env[upper] = "second";
  console.log(
    "case set:",
    env[lower] === "second",
    Object.keys(env).includes(original),
  );
  delete env[lower];
  console.log(
    "case delete:",
    env[upper] === undefined,
    !(upper in env),
    !Object.hasOwn(env, original),
    !Object.keys(env).some((key) => key.toUpperCase() === upper),
  );
} else {
  // Keep the fixture's output stable on Unix, where environment names remain
  // case-sensitive.
  console.log("case get:", true);
  console.log("case has:", true, true);
  console.log("one enumerated spelling:", true);
  console.log("case set:", true, true);
  console.log("case delete:", true, true, true, true);
}
