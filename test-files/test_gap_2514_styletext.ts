// #2514 — util.styleText: wrap text in ANSI codes for named formats.
// Uses {validateStream:false} so output is styled deterministically in a pipe.
import { styleText } from "node:util";

const o = { validateStream: false };
console.log(typeof styleText);
console.log(JSON.stringify(styleText("red", "X", o)));
console.log(JSON.stringify(styleText("green", "ok", o)));
console.log(JSON.stringify(styleText("bold", "b", o)));
console.log(JSON.stringify(styleText("reset", "r", o)));
console.log(JSON.stringify(styleText(["red", "bold"], "rb", o)));
console.log(JSON.stringify(styleText(["bgBlue", "whiteBright", "underline"], "z", o)));
console.log(JSON.stringify(styleText("none", "n", o)));
console.log(JSON.stringify(styleText([], "e", o)));
// default (piped, non-TTY) → unstyled
console.log(JSON.stringify(styleText("red", "plain")));
// errors
try { styleText("notacolor", "x", o); } catch (e) { console.log("badfmt=" + (e as { code?: string }).code); }
try { styleText("red", 5 as unknown as string, o); } catch (e) { console.log("badtext=" + (e as { code?: string }).code); }
try { styleText(["red", "notacolor"], "x", o); } catch (e) { console.log("badarr=" + (e as { code?: string }).code); }
