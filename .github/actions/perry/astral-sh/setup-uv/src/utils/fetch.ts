import {
  EnvHttpProxyAgent,
  type RequestInit,
  fetch as undiciFetch,
} from "undici";

export const fetch = async (url: string, opts: RequestInit) => {
  // Merge timeout signal with any existing signal from opts
  const timeoutSignal = AbortSignal.timeout(5_000);
  const existingSignal = opts.signal;
  const mergedSignal = existingSignal
    ? AbortSignal.any([timeoutSignal, existingSignal])
    : timeoutSignal;

  return await undiciFetch(url, {
    dispatcher: new EnvHttpProxyAgent(),
    ...opts,
    signal: mergedSignal,
  });
};
