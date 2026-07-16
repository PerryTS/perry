import { Session } from "node:inspector";

const session = new Session();
try {
  try {
    session.connectToMainThread();
    console.log("unexpected success");
  } catch (cause) {
    const error = cause as { name?: string; code?: string; message?: string };
    console.log(error.name, error.code, error.message);
  }
  session.connect();
  try {
    session.connectToMainThread();
  } catch (cause) {
    const error = cause as { name?: string; code?: string; message?: string };
    console.log("connected:", error.name, error.code, error.message);
  }
} finally {
  session.disconnect();
}
