- **`http.request` `lookup` option.** HTTP and HTTPS requests now dial
  through the request's `lookup`, `family`, `hints` and `autoSelectFamily`
  options, as Node's agent passes them to `net.createConnection`, so a
  custom resolver runs and the request socket emits `'lookup'`.
