const marks = { true: "[x]", false: "[ ]" };
export function formatTodo({ id, title, done, priority }) {
    return `${String(id).padStart(3)} ${marks[`${done}`]} ${title} (${priority})`;
}
export const HELP = `todo — tiny task list
  add <title> [--priority low|medium|high]
  done <id>
  rm <id>
  ls [all|open|done]`;
