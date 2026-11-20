"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const fs_1 = require("fs");
const types_1 = require("./types");
class TodoStore {
    file;
    todos = [];
    constructor(file) {
        this.file = file;
        if ((0, fs_1.existsSync)(file)) {
            this.todos = JSON.parse((0, fs_1.readFileSync)(file, "utf8"));
        }
    }
    add(title, priority = types_1.Priority.Medium) {
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
        (0, fs_1.writeFileSync)(this.file, JSON.stringify(this.todos, null, 2));
    }
}
exports.default = TodoStore;
