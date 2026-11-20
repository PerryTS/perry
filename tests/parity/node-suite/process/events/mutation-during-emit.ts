const calls: string[] = [];

function second() {
  calls.push("second");
}

function late() {
  calls.push("late");
}

function first() {
  calls.push("first");
  process.removeListener("evt-mutate", second);
  process.on("evt-mutate", late);
}

process.on("evt-mutate", first);
process.on("evt-mutate", second);

process.emit("evt-mutate" as any);
console.log("first emit:", calls.join(","));

calls.length = 0;
process.emit("evt-mutate" as any);
console.log("second emit:", calls.join(","));
console.log("count after second:", process.listenerCount("evt-mutate"));

process.removeAllListeners("evt-mutate");
