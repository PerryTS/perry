`util.MIMEParams` folds the ASCII case of parameter names in `set`, `get`, `has` and `delete`, and has `toJSON`, so `JSON.stringify(params)` prints the serialized parameters as in Node (#15263).
