type MethodName = "includes" | "startsWith" | "endsWith";

function makeCases() {
  const regexpFalse = /a/ as any;
  regexpFalse[Symbol.match] = false;

  return [
    ["regexp", /a/],
    [
      "truthy-match",
      {
        [Symbol.match]: true,
        toString() {
          return "a";
        },
      },
    ],
    [
      "false-match",
      {
        [Symbol.match]: false,
        toString() {
          return "a";
        },
      },
    ],
    [
      "null-match",
      {
        [Symbol.match]: null,
        toString() {
          return "a";
        },
      },
    ],
    [
      "plain-object",
      {
        toString() {
          return "a";
        },
      },
    ],
    ["regexp-false-match", regexpFalse],
    ["undefined", undefined],
  ] as const;
}

function directCall(method: MethodName, arg: any) {
  if (method === "includes") {
    return "abc".includes(arg);
  }
  if (method === "startsWith") {
    return "abc".startsWith(arg);
  }
  return "abc".endsWith(arg);
}

function dynamicCall(method: MethodName, arg: any) {
  const receiver: any = "abc";
  return receiver[method](arg);
}

function logCall(mode: "direct" | "dynamic", method: MethodName, label: string, arg: any) {
  try {
    const result = mode === "direct" ? directCall(method, arg) : dynamicCall(method, arg);
    console.log(mode, method, label, "ok", result);
  } catch (err: any) {
    console.log(mode, method, label, "throw", err.name, err.message, err instanceof TypeError);
  }
}

for (const method of ["includes", "startsWith", "endsWith"] as const) {
  for (const [label, arg] of makeCases()) {
    logCall("direct", method, label, arg);
  }
  for (const [label, arg] of makeCases()) {
    logCall("dynamic", method, label, arg);
  }
}
