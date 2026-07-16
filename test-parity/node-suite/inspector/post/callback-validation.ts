import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  for (const value of [1, true, "callback", {}, []]) {
    try {
      session.post("Runtime.enable", {}, value as never);
      console.log(Array.isArray(value) ? "array" : typeof value, "unexpected");
    } catch (cause) {
      const error = cause as { name?: string; code?: string; message?: string };
      console.log(
        Array.isArray(value) ? "array" : typeof value,
        error.name,
        error.code,
        error.message?.startsWith(
          'The "callback" argument must be of type function.',
        ),
      );
    }
  }
} finally {
  session.disconnect();
}
