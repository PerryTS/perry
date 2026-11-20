### Fixed

- `http.Agent` and `https.Agent` now expose the writable
  `agentKeepAliveTimeoutBuffer` property, retaining finite non-negative
  constructor values and defaulting invalid or missing values to 1000 ms like
  Node.js. This clears the property assertions in Node's
  `test-http-agent-keep-alive-timeout-buffer.js` parity case (#4975).
