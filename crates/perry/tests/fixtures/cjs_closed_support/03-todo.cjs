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
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const os = __importStar(require("os"));
const path_1 = require("path");
const store_1 = __importDefault(require("./lib/store"));
const format_1 = require("./lib/format");
const types_1 = require("./lib/types");
function parseFlags(args) {
    const positional = [];
    const flags = new Map();
    for (let i = 0; i < args.length; i++) {
        const a = args[i];
        if (a.startsWith("--")) {
            flags.set(a.slice(2), args[++i] ?? "");
        }
        else {
            positional.push(a);
        }
    }
    return { positional, flags };
}
const file = process.env.TODO_FILE ?? (0, path_1.join)(os.tmpdir(), "perry-todo.json");
const store = new store_1.default(file);
const [cmd, ...rest] = process.argv.slice(2);
const { positional, flags } = parseFlags(rest);
switch (cmd) {
    case "add": {
        const p = (flags.get("priority") ?? "medium");
        if (!Object.values(types_1.Priority).includes(p)) {
            console.error(`bad priority: ${p}`);
            process.exit(2);
        }
        const t = store.add(positional.join(" "), p);
        console.log(`added #${t.id}`);
        break;
    }
    case "done": {
        const t = store.complete(Number(positional[0]));
        console.log(t ? `completed #${t.id}: ${t.title}` : "no such todo");
        break;
    }
    case "rm":
        console.log(store.remove(Number(positional[0])) ? "removed" : "no such todo");
        break;
    case "ls": {
        const items = store.list(positional[0] ?? "all");
        items.forEach((t) => console.log((0, format_1.formatTodo)(t)));
        console.log(`${items.length} item(s)`);
        break;
    }
    default:
        console.log(format_1.HELP);
}
