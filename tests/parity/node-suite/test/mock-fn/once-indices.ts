import { mock } from "node:test";

const fn = mock.fn(() => "default");
fn();
fn.mock.mockImplementationOnce(() => "third", 2);
fn.mock.mockImplementationOnce(() => "fifth", 4);
console.log("indexed:", fn(), fn(), fn(), fn(), fn());
console.log("count:", fn.mock.callCount());
mock.restoreAll();
