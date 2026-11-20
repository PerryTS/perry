// A local binding named `process` shadows the global: `process.env` reads it.
function env(): string {
  const process = { env: { HOME: "local-home" } as Record<string, string> };
  return process.env.HOME + " " + process.env["HOME"];
}
console.log(env());
