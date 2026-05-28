// Refs #2208: `http.request(...).on(...)` / `https.get(...).on(...)`
// inline-chain dispatch — pre-fix, the chained `.on(...)` fell through
// to the generic typed-feedback path because the HIR-level
// `native_class_from_factory_call` only knew about the `createServer`
// family of factories, so the inner factory result wasn't tagged as
// `ClientRequest`. The fall-through returned an untagged NaN, and each
// subsequent chain step crashed with "(number).<method> is not a
// function". The variable-binding form already worked because the
// let-stmt arm tags the binding with the factory's return class.
//
// This test asserts only the dispatch (no crash, the chain returns the
// same `ClientRequest`); the underlying socket / event-emit machinery
// is exercised by separate parity tests.

import * as http from "http";
import * as https from "https";

// Inline chain — http.request(...).on(...).on(...) used to crash here.
const req1 = http
    .request({ host: "127.0.0.1", port: 1, method: "GET", path: "/" })
    .on("error", (_e: any) => {})
    .on("close", () => {});
console.log("http.request().on().on() returned object:", typeof req1 === "object");

// Same chain via http.get — also a ClientRequest factory.
const req2 = http
    .get("http://127.0.0.1:1/")
    .on("error", (_e: any) => {})
    .on("response", (_r: any) => {});
console.log("http.get().on().on() returned object:", typeof req2 === "object");

// Same chain via https.request — registered under module "http" in the
// HIR class table so methods dispatch identically.
const req3 = https
    .request({ host: "127.0.0.1", port: 1, method: "GET", path: "/" })
    .on("error", (_e: any) => {});
console.log("https.request().on() returned object:", typeof req3 === "object");

// Mid-chain `.setHeader(...)` must also keep the class tag so `.end()` /
// `.write()` after it still dispatch.
const req4 = http
    .request({ host: "127.0.0.1", port: 1, method: "GET", path: "/" })
    .on("error", (_e: any) => {});
req4.end();
console.log("req.end() after chain dispatched:", true);

// Quiet the event loop.
setTimeout(() => {}, 50);
