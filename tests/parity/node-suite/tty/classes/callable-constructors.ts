import tty from "node:tty";

function check(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ":", "no-error");
  } catch (e: any) {
    console.log(label + ":", e?.name, e?.code || "no-code");
  }
}

const ReadStream: any = tty.ReadStream;
const WriteStream: any = tty.WriteStream;

check("ReadStream call -1", () => ReadStream(-1));
check("WriteStream call -1", () => WriteStream(-1));
check("ReadStream new -1", () => new ReadStream(-1));
check("WriteStream new -1", () => new WriteStream(-1));
