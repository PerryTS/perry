var functionExpressionWriter = function () {
  laterFromFunctionExpression = 5;
};
var laterFromFunctionExpression;
functionExpressionWriter();
console.log("function expression:", laterFromFunctionExpression);

function functionDeclarationWriter() {
  laterFromFunctionDeclaration = 7;
}
var laterFromFunctionDeclaration;
functionDeclarationWriter();
console.log("function declaration:", laterFromFunctionDeclaration);

var objectWriter = {
  write() {
    laterFromObjectMethod = 9;
  },
};
var laterFromObjectMethod;
objectWriter.write();
console.log("object method:", laterFromObjectMethod);

var beforeDeclaration;
var beforeWriter = function () {
  beforeDeclaration = 11;
};
beforeWriter();
console.log("declared before writer:", beforeDeclaration);

var initializedWriter = function () {
  laterInitialized = 13;
};
var laterInitialized = 0;
initializedWriter();
console.log("declared later with initializer:", laterInitialized);

topLevelBeforeVar = 17;
var topLevelBeforeVar;
console.log("top-level before var:", topLevelBeforeVar);

if (true) {
  blockBeforeVar = 19;
}
var blockBeforeVar;
console.log("top-level block before var:", blockBeforeVar);
