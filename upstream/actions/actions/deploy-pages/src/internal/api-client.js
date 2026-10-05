const core = require('@actions/core')
const github = require('@actions/github')
const { DefaultArtifactClient } = require('@actions/artifact')
const { RequestError } = require('@octokit/request-error')
const HttpStatusMessages = require('http-status-messages')

function wrapTwirpResponseLikeOctokit(twirpResponse, requestOptions) {
  // Specific response shape aligned with Octokit
  const response = {
    url: requestOptions.url,
    status: 200,
    headers: {
      ...requestOptions.headers
    },
    data: twirpResponse
  }
  return response
}

// Mimic the errors thrown by Octokit for consistency.
function wrapTwirpErrorLikeOctokit(twirpError, requestOptions) {
  const rawErrorMsg = twirpError?.message || twirpError?.toString() || ''
  const statusCodeMatch = rawErrorMsg.match(/Failed request: \((?<statusCode>\d+)\)/)
  const statusCode = statusCodeMatch?.groups?.statusCode ?? 500

  // Try to provide the best error message
  const errorMsg =
    rawErrorMsg ||
    // Fallback to the HTTP status message based on the status code
    HttpStatusMessages[statusCode] ||
    // Or if the status code is unexpected...
    `Unknown error (${statusCode})`

  // RequestError is an Octokit-specific class
  return new RequestError(errorMsg, statusCode, {
    response: {
      url: requestOptions.url,
      status: statusCode,
      headers: {
        ...requestOptions.headers
      },
      data: rawErrorMsg ? { message: rawErrorMsg } : ''
    },
    request: requestOptions
  })
}

function getArtifactsServiceOrigin() {
  const resultsUrl = process.env.ACTIONS_RESULTS_URL
  return resultsUrl ? new URL(resultsUrl).origin : ''
}

async function getArtifactMetadata({ artifactName }) {
  const artifactClient = new DefaultArtifactClient()

  // Primarily for debugging purposes, accuracy is not critical
  const requestOptions = {
    method: 'POST',
    url: `${getArtifactsServiceOrigin()}/twirp/github.actions.results.api.v1.ArtifactService/ListArtifacts`,
    headers: {
      'content-type': 'application/json'
    },
    body: {}
  }

  try {
    core.info(`Fetching artifact metadata for "${artifactName}" in this workflow run`)

    let response
    try {
      const twirpResponse = await artifactClient.listArtifacts()
      response = wrapTwirpResponseLikeOctokit(twirpResponse, requestOptions)
    } catch (twirpError) {
      core.error('Listing artifact metadata failed', twirpError)
      const octokitError = wrapTwirpErrorLikeOctokit(twirpError, requestOptions)
      throw octokitError
    }

    const filteredArtifacts = response.data.artifacts.filter(artifact => artifact.name === artifactName)

    const artifactCount = filteredArtifacts.length
    core.debug(`List artifact count: ${artifactCount}`)

    if (artifactCount === 0) {
      throw new Error(
        `No artifacts named "${artifactName}" were found for this workflow run. Ensure artifacts are uploaded with actions/upload-artifact@v4 or later.`
      )
    } else if (artifactCount > 1) {
      throw new Error(
        `Multiple artifacts named "${artifactName}" were unexpectedly found for this workflow run. Artifact count is ${artifactCount}.`
      )
    }

    const artifact = filteredArtifacts[0]
    core.debug(`Artifact: ${JSON.stringify(artifact)}`)

    if (!artifact.size) {
      core.warning('Artifact size was not found. Unable to verify if artifact size exceeds the allowed size.')
    }

    return artifact
  } catch (error) {
    core.error(
      'Fetching artifact metadata failed. Is githubstatus.com reporting issues with API requests, Pages, or Actions? Please re-run the deployment at a later time.',
      error
    )
    throw error
  }
}

async function createPagesDeployment({ githubToken, artifactId, buildVersion, idToken, isPreview = false }) {
  const octokit = github.getOctokit(githubToken)

  const payload = {
    artifact_id: artifactId,
    pages_build_version: buildVersion,
    oidc_token: idToken
  }
  if (isPreview === true) {
    payload.preview = true
  }
  core.info(`Creating Pages deployment with payload:\n${JSON.stringify(payload, null, '\t')}`)

  try {
    const response = await octokit.request('POST /repos/{owner}/{repo}/pages/deployments', {
      owner: github.context.repo.owner,
      repo: github.context.repo.repo,
      ...payload
    })

    return response.data
  } catch (error) {
    core.error('Creating Pages deployment failed', error)
    throw error
  }
}

async function getPagesDeploymentStatus({ githubToken, deploymentId }) {
  const octokit = github.getOctokit(githubToken)

  core.info('Getting Pages deployment status...')
  try {
    const response = await octokit.request('GET /repos/{owner}/{repo}/pages/deployments/{deploymentId}', {
      owner: github.context.repo.owner,
      repo: github.context.repo.repo,
      deploymentId
    })

    return response.data
  } catch (error) {
    core.error('Getting Pages deployment status failed', error)
    throw error
  }
}

async function cancelPagesDeployment({ githubToken, deploymentId }) {
  const octokit = github.getOctokit(githubToken)

  core.info('Canceling Pages deployment...')
  try {
    const response = await octokit.request('POST /repos/{owner}/{repo}/pages/deployments/{deploymentId}/cancel', {
      owner: github.context.repo.owner,
      repo: github.context.repo.repo,
      deploymentId
    })

    return response.data
  } catch (error) {
    core.error('Canceling Pages deployment failed', error)
    throw error
  }
}

module.exports = {
  getArtifactMetadata,
  createPagesDeployment,
  getPagesDeploymentStatus,
  cancelPagesDeployment
}
