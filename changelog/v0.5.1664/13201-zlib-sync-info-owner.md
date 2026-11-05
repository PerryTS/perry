### Fixed

- Return the genuine processing `Deflate` or `Inflate` owner with the output buffer from `deflateSync` and `inflateSync` when `options.info` is truthy. Preserve consumed-input accounting, option-getter ordering, and bare Buffer results for false or omitted info.
- Add public parity coverage for named, namespace, and captured calls, truthiness, malformed input, and sliced input without treating native codec release as Transform destruction.
