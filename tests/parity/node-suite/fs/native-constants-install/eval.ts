const body = process.argv.length > 0
  ? "return process['get' + 'BuiltinModule']('fs').constants.R_OK"
  : "return 0";
console.log(new Function(body)());
