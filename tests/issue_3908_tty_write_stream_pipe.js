import tty from "node:tty";

function firstLine(err) {
  return String(err?.message || "").split("\n")[0];
}

function check(label, fn) {
  try {
    const value = fn();
    console.log(label + " OK:", value === undefined ? "undefined" : String(value));
  } catch (err) {
    console.log(label + " THROW:", err?.name, err?.code || "no-code", firstLine(err));
  }
}

check("WriteStream fd1 fd", () => new tty.WriteStream(1).fd);
check("WriteStream fd2 fd", () => new tty.WriteStream(2).fd);
check("ReadStream fd0 fd", () => new tty.ReadStream(0).fd);
check("WriteStream invalid -1", () => new tty.WriteStream(-1).fd);
check("ReadStream invalid -1", () => new tty.ReadStream(-1).fd);
