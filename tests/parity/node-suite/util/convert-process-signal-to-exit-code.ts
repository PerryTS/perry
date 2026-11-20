// util.convertProcessSignalToExitCode(signalCode) maps signal names to
// conventional POSIX exit codes and rejects non-signal inputs.
import * as util from "node:util";
import { convertProcessSignalToExitCode } from "node:util";

console.log("typeof:", typeof util.convertProcessSignalToExitCode);
console.log("named equal:", convertProcessSignalToExitCode === util.convertProcessSignalToExitCode);

function show(label: string, value: unknown): void {
  try {
    console.log(label + ":", util.convertProcessSignalToExitCode(value as never));
  } catch (err) {
    const e = err as { name?: string; code?: string };
    console.log(label + " THROW:", e.name, e.code);
  }
}

show("SIGTERM", "SIGTERM");
show("SIGINT", "SIGINT");
show("SIGKILL", "SIGKILL");
show("SIGHUP", "SIGHUP");
show("SIGUSR1", "SIGUSR1");
show("SIGPOLL", "SIGPOLL");
show("SIGFOO", "SIGFOO");
show("sigterm", "sigterm");
show("number", 15);
show("undefined", undefined);
show("null", null);
show("object", {});
show("boolean", true);
