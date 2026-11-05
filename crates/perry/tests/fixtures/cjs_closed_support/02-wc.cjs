"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
// wc-like CLI: counts lines, words, chars of files (or stdin-free sample text).
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
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
