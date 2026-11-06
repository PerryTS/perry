import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  for (const value of [undefined, null, 1, true, {}, []]) {
    try {
      session.post(value as never);
      console.log(typeof value, "unexpected");
    } catch (cause) {
      const error = cause as { name?: string; code?: string; message?: string };
      console.log(
        Array.isArray(value) ? "array" : typeof value,
        error.name,
        error.code,
        error.message?.startsWith(
          'The "method" argument must be of type string.',
        ),
      );
    }
  }
} finally {
  session.disconnect();
}
