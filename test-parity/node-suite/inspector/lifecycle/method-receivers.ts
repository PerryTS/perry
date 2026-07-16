import { Session } from "node:inspector";

for (
  const name of [
    "connect",
    "connectToMainThread",
    "disconnect",
    "post",
  ] as const
) {
  try {
    Reflect.apply(
      Session.prototype[name],
      {},
      name === "post" ? ["Runtime.enable"] : [],
    );
    console.log(name, "unexpected");
  } catch (cause) {
    const error = cause as { name?: string; code?: string; message?: string };
    console.log(
      name,
      error.name,
      error.code ?? "-",
      error.message?.includes("private member") ||
        error.message === "Current thread is not a worker",
    );
  }
}
