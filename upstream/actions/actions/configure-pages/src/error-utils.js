const ErrorStackParser = require('error-stack-parser')

// Convert an Error's stack into `@actions/core` toolkit AnnotationProperties:
// https://github.com/actions/toolkit/blob/ef77c9d60bdb03700d7758b0d04b88446e72a896/packages/core/src/core.ts#L36-L71
function convertErrorToAnnotationProperties(error, title = error.name) {
  if (!(error instanceof Error)) {
    throw new TypeError('error must be an instance of Error')
  }

  const stack = ErrorStackParser.parse(error)
  const firstFrame = stack && stack.length > 0 ? stack[0] : null
  if (!firstFrame) {
    throw new Error('Error stack is empty or unparseable')
  }

  return {
    title,
    file: firstFrame.fileName,
    startLine: firstFrame.lineNumber,
    startColumn: firstFrame.columnNumber
  }
}

module.exports = { convertErrorToAnnotationProperties }
