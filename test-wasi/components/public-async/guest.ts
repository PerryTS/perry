import { register } from "./generated/guest";

register({
  async ping(value: number) { return value + 1; },
});
