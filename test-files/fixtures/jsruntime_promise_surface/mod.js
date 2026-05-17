export function delayedValue(value) {
  return Promise.resolve().then(() => value);
}

export function rejectLater(reason) {
  return Promise.resolve().then(() => {
    throw reason;
  });
}

export function awaitCallback(cb) {
  return Promise.resolve()
    .then(() => cb())
    .then((value) => "callback:" + value);
}

export const moduleValue = await Promise.resolve().then(() => "module-ready");
