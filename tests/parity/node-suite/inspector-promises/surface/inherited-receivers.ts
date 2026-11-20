import { Session } from "node:inspector/promises";

for (const name of ["connect", "disconnect"] as const) {
  try {
    Session.prototype[name].call({});
    console.log(name, "unexpected");
  } catch (error) {
    const cause = error as { name?: string; message?: string };
    console.log(name, cause.name, cause.message?.includes("#connection"));
  }
}
