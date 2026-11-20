// wc-like CLI: counts lines, words, chars of files (or stdin-free sample text).
import * as fs from "fs";
import * as path from "path";
function count(text) {
    const lines = text.length === 0 ? 0 : text.split("\n").length - (text.endsWith("\n") ? 1 : 0);
    const words = text.split(/\s+/).filter((w) => w.length > 0).length;
    return { lines, words, chars: text.length };
}
function pad(n, width = 8) {
    return String(n).padStart(width);
}
const files = process.argv.slice(2);
if (files.length === 0) {
    console.error("usage: wc <file...>");
    process.exit(1);
}
const total = { lines: 0, words: 0, chars: 0 };
for (const file of files) {
    try {
        const text = fs.readFileSync(file, "utf8");
        const c = count(text);
        total.lines += c.lines;
        total.words += c.words;
        total.chars += c.chars;
        console.log(`${pad(c.lines)}${pad(c.words)}${pad(c.chars)} ${path.basename(file)}`);
    }
    catch (e) {
        console.error(`wc: ${file}: ${e.message}`);
        process.exitCode = 1;
    }
}
if (files.length > 1) {
    console.log(`${pad(total.lines)}${pad(total.words)}${pad(total.chars)} total`);
}
