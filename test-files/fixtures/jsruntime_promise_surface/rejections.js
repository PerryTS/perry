export function rejectDirect(reason) {
  return Promise.resolve().then(() => {
    throw reason;
  });
}

let rejectingCallbackCatchCount = 0;

export function resetRejectingCallbackCatchCount() {
  rejectingCallbackCatchCount = 0;
}

export function getRejectingCallbackCatchCount() {
  return rejectingCallbackCatchCount;
}

export function awaitRejectingCallback(cb) {
  return Promise.resolve()
    .then(() => cb())
    .then(() => "missing")
    .catch((reason) => {
      rejectingCallbackCatchCount += 1;
      if (rejectingCallbackCatchCount !== 1) {
        return "native-callback-caught:duplicate:" + rejectingCallbackCatchCount;
      }
      if (reason === undefined) {
        return "native-callback-caught:undefined";
      }
      return "native-callback-caught:" + reason;
    });
}

export function rejectLate(reason) {
  return globalThis.__perryAsyncTick().then(() => {
    throw reason;
  });
}
