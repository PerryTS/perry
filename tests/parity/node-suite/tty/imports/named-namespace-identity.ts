import * as ttyNs from "node:tty";
import { isatty as namedIsatty } from "node:tty";
import tty from "node:tty";

// The named, namespace, and default import bindings must all reach the same
// underlying function reference — Node consolidates them via the module
// namespace object.
console.log("named === namespace:", namedIsatty === ttyNs.isatty);
console.log("default === namespace:", tty.isatty === ttyNs.isatty);
console.log("named === default:", namedIsatty === tty.isatty);
console.log("ReadStream namespace === default:", ttyNs.ReadStream === tty.ReadStream);
console.log("WriteStream namespace === default:", ttyNs.WriteStream === tty.WriteStream);
