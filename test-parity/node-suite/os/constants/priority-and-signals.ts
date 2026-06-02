import os from "node:os";

console.log("SIGINT:", typeof os.constants.signals.SIGINT, os.constants.signals.SIGINT > 0);
console.log("EACCES:", typeof os.constants.errno.EACCES, os.constants.errno.EACCES !== 0);
console.log("priority low:", typeof os.constants.priority.PRIORITY_LOW);
console.log("priority high:", typeof os.constants.priority.PRIORITY_HIGH);
