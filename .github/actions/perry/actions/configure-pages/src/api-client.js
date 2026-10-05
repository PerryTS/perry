const core = require('@actions/core')
const github = require('@actions/github')
const { convertErrorToAnnotationProperties } = require('./error-utils')

async function enablePagesSite({ githubToken }) {
  const octokit = github.getOctokit(githubToken)

  try {
    const response = await octokit.rest.repos.createPagesSite({
      owner: github.context.repo.owner,
      repo: github.context.repo.repo,
      build_type: 'workflow'
    })

    const pageObject = response.data
    return pageObject
  } catch (error) {
    if (error.response && error.response.status === 409) {
      return null
    }

    throw error
  }
}

async function getPagesSite({ githubToken }) {
  const octokit = github.getOctokit(githubToken)

  const response = await octokit.rest.repos.getPages({
    owner: github.context.repo.owner,
    repo: github.context.repo.repo
  })

  const pageObject = response.data
  return pageObject
}

async function findOrCreatePagesSite({ githubToken, enablement = true }) {
  let pageObject

  // Try to find an existing Pages site first
  try {
    pageObject = await getPagesSite({ githubToken })
  } catch (error) {
    if (!enablement) {
      core.error(
        `Get Pages site failed. Please verify that the repository has Pages enabled and configured to build using GitHub Actions, or consider exploring the \`enablement\` parameter for this action. Error: ${error.message}`,
        convertErrorToAnnotationProperties(error)
      )
      throw error
    }
    core.warning(`Get Pages site failed. Error: ${error.message}`, convertErrorToAnnotationProperties(error))
  }

  if (!pageObject && enablement) {
    // Create a new Pages site if one doesn't exist
    try {
      pageObject = await enablePagesSite({ githubToken })
    } catch (error) {
      core.error(`Create Pages site failed. Error: ${error.message}`, convertErrorToAnnotationProperties(error))
      throw error
    }

    // This somehow implies that the Pages site was already created but initially failed to be retrieved.
    // Try one more time for this extreme edge case!
    if (pageObject == null) {
      try {
        pageObject = await getPagesSite({ githubToken })
      } catch (error) {
        core.error(`Get Pages site still failed. Error: ${error.message}`, convertErrorToAnnotationProperties(error))
        throw error
      }
    }
  }

  return pageObject
}

module.exports = { findOrCreatePagesSite, enablePagesSite, getPagesSite }
