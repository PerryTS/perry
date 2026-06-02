import * as tty from "node:tty";

function check(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "threw:", false);
  } catch (e: any) {
    console.log(label, "threw:", true);
    console.log(label, "name:", e?.name);
    console.log(label, "code:", e?.code);
  }
}

// From Node's test-ttywrap-invalid-fd.js: negative fds are rejected before
// attempting platform TTY initialization.
check("WriteStream -1", () => new tty.WriteStream(-1));
check("ReadStream -1", () => new tty.ReadStream(-1));
