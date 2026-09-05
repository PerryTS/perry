import { readDefault, readThunk, Builder } from "./fixtures/issue_9023/defaults.ts";

// The helper's class name must not bind to an unrelated class in the consumer.
class ImportedToken {
 read() { return "consumer"; }
}
console.log("consumer", new ImportedToken().read());
console.log("default", readDefault());
console.log("explicit", readDefault(new ImportedToken()));
console.log("thunk", readThunk());
const builder = new Builder();
console.log("method", builder.make().read());
console.log("method default", builder.read());
