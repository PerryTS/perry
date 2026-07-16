import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  const params: Record<string, unknown> = {};
  params.self = params;
  try {
    session.post("Runtime.evaluate", params);
    console.log("unexpected");
  } catch (cause) {
    const error = cause as { name?: string; message?: string };
    console.log(error.name, error.message?.includes("circular structure"));
  }
} finally {
  session.disconnect();
}
