import { clearLine, clearScreenDown, moveCursor } from "node:readline";
import { Writable } from "node:stream";

// Observe admission through the public helpers, including write side effects.
async function observe(helper: string, mode: string, zeroMovement: boolean) {
  let output = "";
  let writes = 0;
  let callbacks = 0;
  let returned = "not-returned";
  let errorName = "none";
  let errorCode = "none";
  let errorMessage = "none";
  const stream = new Writable({
    write(chunk, _encoding, done) {
      writes++;
      output += chunk;
      done();
    },
  });
  const callback = () => { callbacks++; };
  let supplied: any = undefined;
  if (mode === "callable") supplied = callback;
  if (mode === "string") supplied = "bad";
  if (mode === "null") supplied = null;
  if (mode === "zero") supplied = 0;
  if (mode === "false") supplied = false;
  try {
    if (helper === "clearLine") {
      if (mode === "omitted") returned = String(clearLine(stream, 0));
      else returned = String(clearLine(stream, 0, supplied));
    } else if (helper === "clearScreenDown") {
      if (mode === "omitted") returned = String(clearScreenDown(stream));
      else returned = String(clearScreenDown(stream, supplied));
    } else {
      const dx = zeroMovement ? 0 : 1;
      if (mode === "omitted") returned = String(moveCursor(stream, dx, 0));
      else returned = String(moveCursor(stream, dx, 0, supplied));
    }
  } catch (error: any) {
    errorName = error.name;
    errorCode = error.code;
    errorMessage = error.message;
  }
  // Synchronous Writable completion may defer the public callback to next tick.
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log(helper, mode, zeroMovement, returned, writes,
    JSON.stringify(output), callbacks, errorName, errorCode, errorMessage);
  stream.destroy();
}

for (const helper of ["clearLine", "clearScreenDown", "moveCursor"]) {
  for (const mode of ["omitted", "undefined", "callable", "string", "null", "zero", "false"]) {
    await observe(helper, mode, false);
  }
}
// Admission must precede the zero-movement fast path as well as actual writes.
for (const mode of ["string", "null", "zero", "false"]) {
  await observe("moveCursor", mode, true);
}
