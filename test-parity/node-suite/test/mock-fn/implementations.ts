import { mock } from "node:test";

const fn = mock.fn((value: number) => `original:${value}`);
fn.mock.mockImplementation((value: number) => `replacement:${value}`);
fn.mock.mockImplementationOnce((value: number) => `once-a:${value}`);
fn.mock.mockImplementationOnce((value: number) => `once-b:${value}`);

console.log("sequence:", fn(1), fn(2), fn(3));
console.log("count:", fn.mock.callCount(), fn.mock.calls.length);
fn.mock.resetCalls();
console.log("reset calls:", fn.mock.callCount(), fn(4));
fn.mock.restore();
console.log("restore:", fn(5), fn.mock.callCount());
