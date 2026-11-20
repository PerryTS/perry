const out: any = process.stdout;
for (const name of ["clearLine", "clearScreenDown", "cursorTo", "moveCursor", "getWindowSize"]) {
  console.log(name + ":", typeof out[name]);
}
