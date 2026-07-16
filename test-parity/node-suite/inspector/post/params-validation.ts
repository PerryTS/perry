import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  for (const value of [1, true, "params", Symbol("params")]) {
    try {
      session.post("Runtime.enable", value as never);
      console.log(typeof value, "unexpected");
    } catch (cause) {
      const error = cause as { name?: string; code?: string; message?: string };
      console.log(
        typeof value,
        error.name,
        error.code,
        error.message?.startsWith(
          'The "params" argument must be of type object.',
        ),
      );
    }
  }
} finally {
  session.disconnect();
}
