import { Session } from "node:inspector";

const session = new Session();
for (const label of ["before", "after"] as const) {
  if (label === "after") {
    session.connect();
    session.disconnect();
  }
  try {
    session.post("Runtime.enable");
  } catch (cause) {
    const error = cause as { name?: string; code?: string; message?: string };
    console.log(label, error.name, error.code, error.message);
  }
}
