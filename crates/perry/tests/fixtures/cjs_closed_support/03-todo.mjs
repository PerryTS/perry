import * as os from "os";
import { join } from "path";
import TodoStore from "./lib/store";
import { formatTodo, HELP } from "./lib/format";
import { Priority } from "./lib/types";
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
const file = process.env.TODO_FILE ?? join(os.tmpdir(), "perry-todo.json");
const store = new TodoStore(file);
const [cmd, ...rest] = process.argv.slice(2);
const { positional, flags } = parseFlags(rest);
switch (cmd) {
    case "add": {
        const p = (flags.get("priority") ?? "medium");
        if (!Object.values(Priority).includes(p)) {
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
        items.forEach((t) => console.log(formatTodo(t)));
        console.log(`${items.length} item(s)`);
        break;
    }
    default:
        console.log(HELP);
}
