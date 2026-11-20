// Regression guard: the previous `.code` getter pattern-matched on the
// literal message string `"The argument is invalid"` and applied
// `ERR_INVALID_ARG_TYPE` to any user-thrown TypeError carrying that
// message. After moving to a side-table indexed by the message
// pointer, a user-constructed TypeError with the same message text
// must NOT report a `.code`.
const userError = new TypeError("The argument is invalid");
console.log("user .code:", userError.code === undefined ? "undefined" : userError.code);
console.log("user .message:", userError.message);
console.log("user .name:", userError.name);
