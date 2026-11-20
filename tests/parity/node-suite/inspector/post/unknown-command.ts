import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  await new Promise<void>((resolve) => {
    session.post("Missing.command", {}, (cause, result) => {
      const error = cause as unknown as {
        name?: string;
        code?: string;
        message?: string;
      } | null;
      console.log(
        error?.name ?? "<none>",
        error?.code ?? "<none>",
        error?.message ?? "<none>",
        result === undefined,
      );
      resolve();
    });
  });
} finally {
  session.disconnect();
}
