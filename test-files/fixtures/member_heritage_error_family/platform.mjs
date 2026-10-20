// Effect's PlatformError.js shape: a package class whose heritage is a member
// named `Error` that is not the global Error.
import * as ns from "./ns.mjs";

export class SystemError extends ns.Error {
  get message() {
    return `${this._tag}: ${this.module}.${this.method}`;
  }
}
