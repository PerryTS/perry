// styleText's default stream check (node's shouldColorize(process.stdout)):
// run by run_style_text_color_tests.sh under a pty and through a pipe, with
// the colour environment variables set per case. Every line is one string, so
// console.log adds no colour of its own.
import { styleText } from "node:util";

console.log("isTTY " + String(process.stdout.isTTY === true));
console.log("red " + JSON.stringify(styleText("red", "x")));
console.log("bold-green " + JSON.stringify(styleText(["bold", "green"], "ok")));
console.log("stream-check-off " + JSON.stringify(styleText("red", "x", { validateStream: false })));
