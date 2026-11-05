// Effect's FileSystem.js shape: another module constructs the imported class.
import { SystemError } from "./platform.mjs";

export const notFound = (path) =>
  new SystemError({ _tag: "NotFound", module: "FS", method: "stat", path });
