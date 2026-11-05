import tty from "node:tty";

const ReadStream: any = tty.ReadStream;
const WriteStream: any = tty.WriteStream;

const rs: any = ReadStream(0);
console.log("read instanceof:", rs instanceof tty.ReadStream);
console.log("read isTTY:", rs.isTTY === true);
console.log("read raw initial:", rs.isRaw === false);
console.log("read setRaw true self:", rs.setRawMode(true) === rs);
console.log("read raw true:", rs.isRaw === true);
console.log("read setRaw false self:", rs.setRawMode(false) === rs);
console.log("read raw false:", rs.isRaw === false);

const ws: any = WriteStream(1);
console.log("write instanceof:", ws instanceof tty.WriteStream);
console.log("write isTTY:", ws.isTTY === true);
console.log("write dimensions:", typeof ws.columns === "number", typeof ws.rows === "number");
const size = ws.getWindowSize();
console.log("window size:", Array.isArray(size), typeof size[0], typeof size[1]);
console.log("cursorTo return:", ws.cursorTo(0) === true);
console.log("moveCursor return:", ws.moveCursor(1, 0) === true);
console.log("clearLine return:", ws.clearLine(0) === true);
console.log("clearScreenDown return:", ws.clearScreenDown() === true);
process.stdout.write("\n");

let resizeCount = 0;
console.log("resize listener self:", ws.addListener("resize", () => {}) === ws);
ws.removeListener("resize", () => {});
process.stdout.on("resize", () => {
  resizeCount += 1;
  console.log("resize event:", resizeCount, typeof process.stdout.columns, typeof process.stdout.rows);
});

setTimeout(() => {
  console.log("resize count final:", resizeCount >= 1);
  rs.setRawMode(false);
}, 1000);
