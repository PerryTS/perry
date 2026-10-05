const core = require('@actions/core')

// Load variables from Actions runtime
function getRequiredVars() {
  return {
    workflowRun: process.env.GITHUB_RUN_ID,
    repositoryNwo: process.env.GITHUB_REPOSITORY,
    buildVersion: process.env.GITHUB_SHA,
    buildActor: process.env.GITHUB_ACTOR,
    actionsId: process.env.GITHUB_ACTION,
    githubToken: core.getInput('token'),
    githubApiUrl: process.env.GITHUB_API_URL ?? 'https://api.github.com',
    githubServerUrl: process.env.GITHUB_SERVER_URL ?? 'https://github.com',
    artifactName: core.getInput('artifact_name') || 'github-pages',
    isPreview: core.getInput('preview') === 'true'
  }
}

module.exports = function getContext() {
  const requiredVars = getRequiredVars()
  for (const variable in requiredVars) {
    if (requiredVars[variable] === undefined) {
      throw new Error(`${variable} is undefined. Cannot continue.`)
    }
  }
  core.debug('all variables are set')
  return requiredVars
}
