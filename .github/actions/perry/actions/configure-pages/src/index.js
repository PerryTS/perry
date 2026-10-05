const core = require('@actions/core')

// All variables we need from the runtime are loaded here
const { getContext } = require('./context')

const { findOrCreatePagesSite } = require('./api-client')
const { setPagesConfig } = require('./set-pages-config')
const outputPagesBaseUrl = require('./output-pages-base-url')

async function main() {
  try {
    const { githubToken, enablement, staticSiteGenerator, generatorConfigFile } = getContext()

    const pageObject = await findOrCreatePagesSite({ githubToken, enablement })
    const siteUrl = new URL(pageObject.html_url)

    if (staticSiteGenerator) {
      setPagesConfig({ staticSiteGenerator, generatorConfigFile, siteUrl })
    }
    outputPagesBaseUrl(siteUrl)
    core.exportVariable('GITHUB_PAGES', 'true')
  } catch (error) {
    core.setFailed(error)
    process.exit(1)
  }
}

// Main
main()
