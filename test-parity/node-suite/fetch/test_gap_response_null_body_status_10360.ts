// #10360: a Response init with a null-body status (204/205/304) and a
// non-null body is a TypeError in Node for BOTH `new Response(...)` and
// `Response.json(...)`; `Response.json` also shares the constructor's status
// range and statusText checks. (Bun accepts the null-body case — Perry
// follows that only under `--platform bun`, covered by
// crates/perry/tests/issue_10360_bun_platform_response_null_body.rs.)
const t = (label: string, f: () => any) => {
  try {
    const v = f();
    console.log(label, v === undefined ? "undefined" : JSON.stringify(v));
  } catch (e: any) {
    console.log(label, "THREW " + e.constructor.name + ": " + e.message);
  }
};

// --- constructor: null / undefined body is fine under any status ---
t("ctor null 204", () => new Response(null, { status: 204 }).status);
t("ctor undefined 204", () => new Response(undefined, { status: 204 }).status);
t("ctor null 304", () => new Response(null, { status: 304 }).status);
t("ctor no-body 205", () => new Response(undefined, { status: 205, statusText: "Reset" }).statusText);

// --- constructor: non-null body + null-body status throws ---
t("ctor 'x' 204", () => new Response("x", { status: 204 }).status);
t("ctor 'x' 205", () => new Response("x", { status: 205 }).status);
t("ctor 'x' 304", () => new Response("x", { status: 304 }).status);
t("ctor '' 204", () => new Response("", { status: 204 }).status);

// --- constructor: same body, other statuses ---
t("ctor 'x' 201", () => new Response("x", { status: 201 }).status);
t("ctor 'x' 599", () => new Response("x", { status: 599 }).status);
t("ctor '' 200", () => new Response("", { status: 200 }).status);

// --- Response.json: always has a body, so a null-body status throws ---
t("json 204", () => Response.json({ a: 1 }, { status: 204 }).status);
t("json 205", () => Response.json({ a: 1 }, { status: 205 }).status);
t("json 304", () => Response.json(null, { status: 304 }).status);
const noContent = { status: 204 };
t("json 204 via variable init", () => Response.json({ a: 1 }, noContent).status);

// --- Response.json: shares the constructor's status range / statusText checks ---
t("json 99", () => Response.json({}, { status: 99 }).status);
t("json 600", () => Response.json({}, { status: 600 }).status);
t("json 199.9", () => Response.json({}, { status: 199.9 }).status);
t("json 599.9", () => Response.json({}, { status: 599.9 }).status);
t("json bad statusText", () => Response.json({}, { statusText: "bad\nline" }).status);
const badStatus = { status: 600 };
t("json 600 via variable init", () => Response.json({}, badStatus).status);

// --- Response.json: valid inits still work ---
t("json default", () => Response.json({ a: 1 }).status);
t("json 201", () => {
  const r = Response.json({ a: 1 }, { status: 201, statusText: "Created" });
  return [r.status, r.statusText, r.headers.get("content-type")];
});
