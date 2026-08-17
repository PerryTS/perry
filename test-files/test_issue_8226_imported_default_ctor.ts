import {
  ColumnWithAutoIncrement,
  getConstructorCalls,
} from "./issue_8226_base.ts";

class IntColumn extends ColumnWithAutoIncrement {}

const config = {
  name: "id",
  uniqueName: undefined as string | undefined,
  autoIncrement: true,
};
const column = new IntColumn("users", config);

console.log(config.uniqueName);
console.log(column.autoIncrement);
console.log(getConstructorCalls());
