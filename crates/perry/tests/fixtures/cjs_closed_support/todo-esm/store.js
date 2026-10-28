import { existsSync, readFileSync, writeFileSync } from "fs";
import { Priority } from "./types";
export default class TodoStore {
    file;
    todos = [];
    constructor(file) {
        this.file = file;
        if (existsSync(file)) {
            this.todos = JSON.parse(readFileSync(file, "utf8"));
        }
    }
    add(title, priority = Priority.Medium) {
        const id = this.todos.reduce((max, t) => Math.max(max, t.id), 0) + 1;
        const todo = { id, title, done: false, priority, createdAt: new Date().toISOString() };
        this.todos.push(todo);
        this.save();
        return todo;
    }
    complete(id) {
        const todo = this.todos.find((t) => t.id === id);
        if (todo) {
            todo.done = true;
            this.save();
        }
        return todo;
    }
    remove(id) {
        const before = this.todos.length;
        this.todos = this.todos.filter((t) => t.id !== id);
        this.save();
        return this.todos.length < before;
    }
    list(filter = "all") {
        const order = { high: 0, medium: 1, low: 2 };
        return this.todos
            .filter((t) => (filter === "all" ? true : filter === "done" ? t.done : !t.done))
            .sort((a, b) => order[a.priority] - order[b.priority] || a.id - b.id);
    }
    save() {
        writeFileSync(this.file, JSON.stringify(this.todos, null, 2));
    }
}
