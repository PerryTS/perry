import { Session } from "node:inspector/promises";

const session = new Session();
try {
  const connected = session.connect();
  console.log(
    "connect:",
    connected === undefined,
    connected instanceof Promise,
  );
  try {
    session.connect();
    console.log("unexpected second connect");
  } catch (error) {
    const cause = error as { name?: string; code?: string };
    console.log("second connect:", cause.name, cause.code);
  }
  const disconnected = session.disconnect();
  const repeated = session.disconnect();
  console.log(
    "disconnect:",
    disconnected === undefined,
    disconnected instanceof Promise,
    repeated === undefined,
  );
  try {
    session.connectToMainThread();
    console.log("unexpected main-thread connection");
  } catch (error) {
    const cause = error as { name?: string; code?: string };
    console.log("main thread:", cause.name, cause.code);
  }
} finally {
  session.disconnect();
}
