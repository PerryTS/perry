import { parentPort, workerData, markAsUncloneable } from "node:worker_threads";
class Issue {
    ast = { run: () => 1 };
}
class SchemaError extends Error {
    name = "SchemaError";
    issue = new Issue();
}
class DerivedTypeError extends TypeError {
    issue = new Issue();
}
parentPort!.on("message", () => {
    let error: any;
    if (workerData === "subclass") error = new SchemaError("schema failed");
    else if (workerData === "type-subclass") error = new DerivedTypeError("type schema failed");
    else if (workerData === "class") error = new TypeError("class property");
    else if (workerData === "cause") error = new Error("with cause", { cause: { reason: "inner" } });
    else if (workerData === "bad-cause") error = new Error("bad cause", { cause: () => 1 });
    else error = new Error("function property");
    if (workerData !== "unmarked") error.stack = "stable worker stack";
    if (workerData !== "bad-cause" && workerData !== "unmarked") markAsUncloneable(error);
    if (workerData === "class") error.issue = new Issue();
    else error.extra = () => 1;
    throw error;
});
