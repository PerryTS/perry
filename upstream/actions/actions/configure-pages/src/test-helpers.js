const fs = require('fs')
const prettier = require('prettier')
const assert = require('assert')

// Create and return the path to a temp folder
function getTempFolder() {
  const tmpFolder = `${__dirname}/fixtures/tmp`
  if (!fs.existsSync(tmpFolder)) {
    fs.mkdirSync(tmpFolder, { recursive: true })
  }
  return tmpFolder
}

// Read a JavaScript file and return it formatted
function formatFile(file) {
  const fileContent = fs.readFileSync(file, 'utf8')
  return prettier
    .format(fileContent, {
      parser: 'espree',

      // Prettier options
      printWidth: 120,
      tabWidth: 2,
      useTabs: false,
      semi: false,
      singleQuote: true,
      trailingComma: 'none',
      bracketSpacing: true,
      arrowParens: 'avoid'
    })
    .trim()
}

// Compare two JavaScript files
function compareFiles(actualFile, expectedFile) {
  assert.equal(formatFile(actualFile), formatFile(expectedFile))
}

module.exports = { getTempFolder, formatFile, compareFiles }
