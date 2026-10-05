const fs = require('fs')
const path = require('path')
const core = require('@actions/core')

const { setPagesConfig } = require('./set-pages-config')
const { getTempFolder, compareFiles } = require('./test-helpers')

// Get the temp folder
const tempFolder = getTempFolder()

const SUPPORTED_GENERATORS = ['next', 'nuxt', 'gatsby', 'sveltekit']
const SUPPORTED_FILE_EXTENSIONS = ['.js', '.cjs', '.mjs']
const IS_BLANK_CONFIG_FILE_REGEX = new RegExp(
  '^blank\\.(' + SUPPORTED_FILE_EXTENSIONS.map(ext => ext.slice(1)).join('|') + ')$'
)

function isBlankConfigFileName(fileName) {
  return IS_BLANK_CONFIG_FILE_REGEX.test(fileName)
}

// Test suite
describe('configParser', () => {
  beforeEach(() => {
    jest.restoreAllMocks()

    // Mock error/warning/info/debug to silence their output
    jest.spyOn(core, 'error').mockImplementation(jest.fn())
    jest.spyOn(core, 'warning').mockImplementation(jest.fn())
    jest.spyOn(core, 'info').mockImplementation(jest.fn())
    jest.spyOn(core, 'debug').mockImplementation(jest.fn())
  })

  // Iterate over the static site generators
  SUPPORTED_GENERATORS.forEach(staticSiteGenerator => {
    // Folder containing the fixtures for a given static site generator
    const fixtureFolder = `${__dirname}/fixtures/${staticSiteGenerator}`

    // Get fixture files, excluding expected results
    const configurationFiles = fs.readdirSync(fixtureFolder).filter(filename => !filename.includes('.expected.'))

    // Create test siteUrl
    const siteUrl = new URL('https://configure-pages.github.io/docs/')

    // Iterate over the fixtures, outputting to default configuration file path
    const defaultFileExtension = '.js'
    configurationFiles
      .filter(filename => filename.endsWith(defaultFileExtension))
      .forEach(configurationFile => {
        it(`injects path properly for ${staticSiteGenerator} in ${configurationFile} to default configuration file`, async () => {
          // Copy the source fixture to a temp file
          const fixtureSourceFile = `${fixtureFolder}/${configurationFile}`
          const fixtureTargetFile = `${tempFolder}/${configurationFile}`
          if (!isBlankConfigFileName(configurationFile)) {
            fs.copyFileSync(fixtureSourceFile, fixtureTargetFile)
          } else if (fs.existsSync(fixtureTargetFile)) {
            fs.rmSync(fixtureTargetFile)
          }

          // Do the injections for the static site generator
          setPagesConfig({
            staticSiteGenerator,
            generatorConfigFile: fixtureTargetFile,
            siteUrl
          })

          // Read the expected file
          const expectedFile = `${fixtureFolder}/${path.basename(
            configurationFile,
            defaultFileExtension
          )}.expected${defaultFileExtension}`

          // Compare the actual and expected files
          compareFiles(fixtureTargetFile, expectedFile)
        })
      })

    SUPPORTED_FILE_EXTENSIONS.forEach(fileExtension => {
      // Iterate over the fixtures, outputting to specified configuration file path
      configurationFiles
        .filter(filename => filename.endsWith(fileExtension))
        .forEach(configurationFile => {
          it(`injects path properly for ${staticSiteGenerator} in ${configurationFile} to specified *${fileExtension} configuration file`, async () => {
            // Copy the source fixture to a temp file
            const fixtureSourceFile = `${fixtureFolder}/${configurationFile}`
            const fixtureTargetFile = `${tempFolder}/${configurationFile}`
            if (!isBlankConfigFileName(configurationFile)) {
              fs.copyFileSync(fixtureSourceFile, fixtureTargetFile)
            } else if (fs.existsSync(fixtureTargetFile)) {
              fs.rmSync(fixtureTargetFile)
            }

            // Do the injections for the static site generator
            setPagesConfig({
              staticSiteGenerator,
              generatorConfigFile: fixtureTargetFile,
              siteUrl
            })

            // Read the expected file
            const expectedFile = `${fixtureFolder}/${path.basename(
              configurationFile,
              fileExtension
            )}.expected${fileExtension}`

            // Compare the actual and expected files
            compareFiles(fixtureTargetFile, expectedFile)
          })
        })
    })
  })
})
