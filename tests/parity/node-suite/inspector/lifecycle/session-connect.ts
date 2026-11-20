import { Session } from "node:inspector";

function attempt(label: string, fn: () => unknown): void {
  try {
    console.log(label, "ok", fn() === undefined);
  } catch (cause) {
    const error = cause as { name?: string; code?: string; message?: string };
    console.log(label, "err", error.name, error.code, error.message);
  }
}

const session = new Session();
try {
  attempt("disconnect fresh:", () => session.disconnect());
  attempt("connect:", () => session.connect());
  attempt("connect twice:", () => session.connect());
  attempt("disconnect:", () => session.disconnect());
  attempt("disconnect twice:", () => session.disconnect());
  attempt("reconnect:", () => session.connect());
  attempt("disconnect final:", () => session.disconnect());
} finally {
  session.disconnect();
}
