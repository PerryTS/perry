import { Readline } from "node:readline/promises";

for (const value of [undefined, null, 0, true, "", {}, []]) {
  try {
    new Readline(value as any);
    console.log("ok");
  } catch (error: any) {
    console.log(error.name, error.code);
  }
}
