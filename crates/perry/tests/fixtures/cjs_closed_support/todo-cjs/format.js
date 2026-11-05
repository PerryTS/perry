"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.HELP = void 0;
exports.formatTodo = formatTodo;
const marks = { true: "[x]", false: "[ ]" };
function formatTodo({ id, title, done, priority }) {
    return `${String(id).padStart(3)} ${marks[`${done}`]} ${title} (${priority})`;
}
exports.HELP = `todo — tiny task list
  add <title> [--priority low|medium|high]
  done <id>
  rm <id>
  ls [all|open|done]`;
