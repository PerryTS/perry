const core = require('@actions/core')
const removeTrailingSlash = require('./remove-trailing-slash')

function outputPagesBaseUrl(siteUrl) {
  // Many static site generators do not want the trailing slash, and it is much easier to add than remove in a workflow
  const baseUrl = removeTrailingSlash(siteUrl.href)
  const basePath = removeTrailingSlash(siteUrl.pathname)

  core.setOutput('base_url', baseUrl)
  core.setOutput('origin', siteUrl.origin)
  core.setOutput('host', siteUrl.host)
  core.setOutput('base_path', basePath)
}

module.exports = outputPagesBaseUrl
