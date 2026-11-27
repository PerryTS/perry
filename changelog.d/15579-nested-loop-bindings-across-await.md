Async functions now preserve nested loop bindings across `await` while keeping writable captures separate for each iteration. This fixes a startup failure in NestJS applications.
