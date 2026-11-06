import { CODE } from "./util.ts";
console.log("err init");
export class ParseError {
  code(): number {
    return CODE + 2;
  }
}
