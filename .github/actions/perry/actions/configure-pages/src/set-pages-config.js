const fs = require('fs')
const core = require('@actions/core')
const { ConfigParser } = require('./config-parser')
const removeTrailingSlash = require('./remove-trailing-slash')
const { convertErrorToAnnotationProperties } = require('./error-utils')

const SUPPORTED_FILE_EXTENSIONS = ['.js', '.cjs', '.mjs']

function detectOrDefaultConfigFile(fileBaseName, defaultExt = '.js') {
  for (const ext of SUPPORTED_FILE_EXTENSIONS) {
    const potentialConfigFile = `./${fileBaseName}${ext}`
    if (fs.existsSync(potentialConfigFile)) {
      return potentialConfigFile
    }
  }
  // If none of them exist yet, fall back to returning the filename with the defaultExt extension
  return `./${fileBaseName}${defaultExt}`
}

// Return the settings to be passed to a {ConfigParser} for a given static site generator,
// optional configuration file path, and a Pages siteUrl value to inject
function getConfigParserSettings({ staticSiteGenerator, generatorConfigFile, siteUrl }) {
  let { pathname: path, origin } = siteUrl

  switch (staticSiteGenerator) {
    case 'nuxt':
      return {
        configurationFile: generatorConfigFile || detectOrDefaultConfigFile('nuxt.config'),
        blankConfigurationFile: `${__dirname}/blank-configurations/nuxt.js`,
        properties: {
          // Configure a base path on the router
          'router.base': path,

          // Set the target to static too
          // https://nuxtjs.org/docs/configuration-glossary/configuration-target/
          target: 'static'
        }
      }
    case 'next':
      // Next does not want a trailing slash
      path = removeTrailingSlash(path)

      return {
        configurationFile: generatorConfigFile || detectOrDefaultConfigFile('next.config'),
        blankConfigurationFile: `${__dirname}/blank-configurations/next.js`,
        properties: {
          // Static export
          // https://nextjs.org/docs/app/building-your-application/deploying/static-exports#version-history
          output: 'export',

          // Configure a base path
          // https://nextjs.org/docs/app/api-reference/next-config-js/basePath
          basePath: path,

          // Disable server side image optimization too
          // https://nextjs.org/docs/api-reference/next/image#unoptimized
          'images.unoptimized': true
        }
      }
    case 'gatsby':
      return {
        configurationFile: generatorConfigFile || detectOrDefaultConfigFile('gatsby-config'),
        blankConfigurationFile: `${__dirname}/blank-configurations/gatsby.js`,
        properties: {
          // Configure a path prefix
          pathPrefix: path,
          // Configure a site url
          'siteMetadata.siteUrl': origin
        }
      }
    case 'sveltekit':
      // SvelteKit does not want a trailing slash
      path = removeTrailingSlash(path)

      return {
        configurationFile: generatorConfigFile || detectOrDefaultConfigFile('svelte.config'),
        blankConfigurationFile: `${__dirname}/blank-configurations/sveltekit.js`,
        properties: {
          // Configure a base path
          'kit.paths.base': path,
          // Configure a prerender origin
          'kit.prerender.origin': origin
        }
      }
    default:
      throw `Unsupported static site generator: ${staticSiteGenerator}`
  }
}

// Inject Pages configuration in a given static site generator's configuration file
function setPagesConfig({ staticSiteGenerator, generatorConfigFile, siteUrl }) {
  const isSupportedFileExtension = SUPPORTED_FILE_EXTENSIONS.some(ext => generatorConfigFile.endsWith(ext))
  if (generatorConfigFile && !isSupportedFileExtension) {
    const supportedExtensionList = SUPPORTED_FILE_EXTENSIONS.map(ext => JSON.stringify(ext)).join(', ')
    core.warning(
      `Unsupported extension in configuration file: ${generatorConfigFile}. Currently supported extensions: ${supportedExtensionList}. We will still attempt to inject the site metadata into the configuration file, but it may not work as expected.`
    )
  }

  try {
    // Parse the configuration file and try to inject the Pages configuration in it
    const settings = getConfigParserSettings({ staticSiteGenerator, generatorConfigFile, siteUrl })
    new ConfigParser(settings).injectAll()
  } catch (error) {
    core.warning(
      `We were unable to determine how to inject the site metadata into your config. Generated URLs may be incorrect. The base URL for this site should be ${siteUrl}. Please ensure your framework is configured to generate relative links appropriately. Error: ${error.message}`,
      convertErrorToAnnotationProperties(error)
    )
  }
}

module.exports = { getConfigParserSettings, setPagesConfig }
