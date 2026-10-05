const core = require('@actions/core')
// For mocking network calls with core http (http-client)
const nock = require('nock')
// For mocking network calls with native Fetch (octokit)
const { MockAgent, setGlobalDispatcher } = require('undici')

const {
  Deployment,
  MAX_TIMEOUT,
  DEFAULT_REPORTING_INTERVAL,
  MAX_REPORTING_INTERVAL,
  ONE_GIGABYTE,
  SIZE_LIMIT_DESCRIPTION
} = require('../../internal/deployment')

const fakeJwt =
  'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJqdGkiOiJiNjllMWIxOC1jOGFiLTRhZGQtOGYxOC03MzVlMzVjZGJhZjAiLCJzdWIiOiJyZXBvOnBhcGVyLXNwYS9taW55aTplbnZpcm9ubWVudDpQcm9kdWN0aW9uIiwiYXVkIjoiaHR0cHM6Ly9naXRodWIuY29tL3BhcGVyLXNwYSIsInJlZiI6InJlZnMvaGVhZHMvbWFpbiIsInNoYSI6ImEyODU1MWJmODdiZDk3NTFiMzdiMmM0YjM3M2MxZjU3NjFmYWM2MjYiLCJyZXBvc2l0b3J5IjoicGFwZXItc3BhL21pbnlpIiwicmVwb3NpdG9yeV9vd25lciI6InBhcGVyLXNwYSIsInJ1bl9pZCI6IjE1NDY0NTkzNjQiLCJydW5fbnVtYmVyIjoiMzQiLCJydW5fYXR0ZW1wdCI6IjIiLCJhY3RvciI6IllpTXlzdHkiLCJ3b3JrZmxvdyI6IkNJIiwiaGVhZF9yZWYiOiIiLCJiYXNlX3JlZiI6IiIsImV2ZW50X25hbWUiOiJwdXNoIiwicmVmX3R5cGUiOiJicmFuY2giLCJlbnZpcm9ubWVudCI6IlByb2R1Y3Rpb24iLCJqb2Jfd29ya2Zsb3dfcmVmIjoicGFwZXItc3BhL21pbnlpLy5naXRodWIvd29ya2Zsb3dzL2JsYW5rLnltbEByZWZzL2hlYWRzL21haW4iLCJpc3MiOiJodHRwczovL3Rva2VuLmFjdGlvbnMuZ2l0aHVidXNlcmNvbnRlbnQuY29tIiwibmJmIjoxNjM4ODI4MDI4LCJleHAiOjE2Mzg4Mjg5MjgsImlhdCI6MTYzODgyODYyOH0.1wyupfxu1HGoTyIqatYg0hIxy2-0bMO-yVlmLSMuu2w'

const LIST_ARTIFACTS_TWIRP_PATH = '/twirp/github.actions.results.api.v1.ArtifactService/ListArtifacts'

describe('Deployment', () => {
  let mockPool

  beforeEach(() => {
    jest.clearAllMocks()
    process.env.GITHUB_RUN_ID = '123'
    process.env.GITHUB_REPOSITORY = 'actions/is-awesome'
    process.env.GITHUB_TOKEN = 'gha-token'
    process.env.GITHUB_SHA = '123abc'
    process.env.GITHUB_ACTOR = 'monalisa'
    process.env.GITHUB_ACTION = '__monalisa/octocat'
    process.env.GITHUB_ACTION_PATH = 'something'
    // A valid actions token must have an 'scp' field whose value is a space-delimited list of strings
    process.env.ACTIONS_RUNTIME_TOKEN =
      'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzY3AiOiJBY3Rpb25zLkV4YW1wbGVTY29wZSBBY3Rpb25zLlJlc3VsdHM6Y2U3ZjU0YzctNjFjNy00YWFlLTg4N2YtMzBkYTQ3NWY1ZjFhOmNhMzk1MDg1LTA0MGEtNTI2Yi0yY2U4LWJkYzg1ZjY5Mjc3NCJ9.l-VcBU1PeNk_lWpOhjWehQlYyjCcY2dp_EMt7Rf06io'
    process.env.ACTIONS_RESULTS_URL = 'https://actions-results-url.biz'

    jest.spyOn(core, 'getInput').mockImplementation(param => {
      switch (param) {
        case 'artifact_name':
          return 'github-pages'
        case 'token':
          return process.env.GITHUB_TOKEN
        case 'reporting_interval':
          return process.env.INPUT_REPORTING_INTERVAL || 50 // Lower reporting interval to speed up test
        default:
          return process.env[`INPUT_${param.toUpperCase()}`] || ''
      }
    })

    jest.spyOn(core, 'setOutput').mockImplementation(param => {
      return param
    })

    jest.spyOn(core, 'setFailed').mockImplementation(param => {
      return param
    })

    // Mock error/warning/info/debug
    jest.spyOn(core, 'error').mockImplementation(jest.fn())
    jest.spyOn(core, 'warning').mockImplementation(jest.fn())
    jest.spyOn(core, 'info').mockImplementation(jest.fn())
    jest.spyOn(core, 'debug').mockImplementation(jest.fn())

    // Set up Fetch mocking
    let mockAgent = new MockAgent()
    mockAgent.disableNetConnect()
    setGlobalDispatcher(mockAgent)
    mockPool = mockAgent.get('https://api.github.com')
  })

  describe('#create', () => {
    afterEach(() => {
      // Remove mock for `core.getInput('preview')`
      delete process.env.INPUT_PREVIEW
    })

    it('can successfully create a deployment', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.pages_build_version === process.env.GITHUB_SHA &&
              body.oidc_token === fakeJwt
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)

      expect(core.setFailed).not.toHaveBeenCalled()
      expect(core.info).toHaveBeenLastCalledWith(
        expect.stringMatching(new RegExp(`^Created deployment for ${process.env.GITHUB_SHA}`))
      )
      twirpScope.done()
    })

    it('can successfully create a deployment with a 64-character build version', async () => {
      process.env.GITHUB_SHA = 'a'.repeat(64)

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.pages_build_version === process.env.GITHUB_SHA &&
              body.oidc_token === fakeJwt
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      const deployment = new Deployment()
      await deployment.create(fakeJwt)

      expect(process.env.GITHUB_SHA).toHaveLength(64)
      expect(core.setFailed).not.toHaveBeenCalled()
      expect(core.info).toHaveBeenLastCalledWith(
        expect.stringMatching(new RegExp(`^Created deployment for ${process.env.GITHUB_SHA}`))
      )

      twirpScope.done()
    })

    it('can successfully create a preview deployment', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 4 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              keys[3] === 'preview' &&
              body.artifact_id === 11 &&
              body.pages_build_version === process.env.GITHUB_SHA &&
              body.oidc_token === fakeJwt &&
              body.preview === true
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome',
            preview_url: 'https://actions.drafts.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Return `"true"` for `core.getInput("preview")`
      process.env.INPUT_PREVIEW = 'true'

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)

      expect(core.setFailed).not.toHaveBeenCalled()
      expect(core.info).toHaveBeenLastCalledWith(
        expect.stringMatching(new RegExp(`^Created deployment for ${process.env.GITHUB_SHA}`))
      )
      twirpScope.done()
    })

    it('reports errors with failed artifact metadata exchange', async () => {
      process.env.GITHUB_SHA = 'invalid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(400, { msg: 'yikes!' }, { 'content-type': 'application/json' })

      // Create the deployment
      const deployment = new Deployment()
      await expect(deployment.create()).rejects.toThrow(
        `Failed to create deployment (status: 400) with build version ${process.env.GITHUB_SHA}.`
      )
      expect(core.error).toHaveBeenNthCalledWith(
        1,
        'Listing artifact metadata failed',
        new Error('Failed to ListArtifacts: Received non-retryable error: Failed request: (400) null: yikes!')
      )
      expect(core.error).toHaveBeenNthCalledWith(
        2,
        'Fetching artifact metadata failed. Is githubstatus.com reporting issues with API requests, Pages, or Actions? Please re-run the deployment at a later time.',
        expect.any(Error)
      )
      twirpScope.done()
    })

    it('reports errors with a failed 500 in a deployment', async () => {
      process.env.GITHUB_SHA = 'build-version'
      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 2 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(500, { message: 'oh no' }, { headers: { 'content-type': 'application/json' } })

      // Create the deployment
      const deployment = new Deployment()
      await expect(deployment.create()).rejects.toEqual(
        new Error(
          `Failed to create deployment (status: 500) with build version ${process.env.GITHUB_SHA}. Server error, is githubstatus.com reporting a Pages outage? Please re-run the deployment at a later time.`
        )
      )
      twirpScope.done()
    })

    it('reports errors with an unexpected 403 during deployment', async () => {
      process.env.GITHUB_SHA = 'build-version'
      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 2 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(403, { message: 'You are forbidden' }, { headers: { 'content-type': 'application/json' } })

      // Create the deployment
      const deployment = new Deployment()
      await expect(deployment.create()).rejects.toEqual(
        new Error(
          `Failed to create deployment (status: 403) with build version ${process.env.GITHUB_SHA}. Ensure GITHUB_TOKEN has permission "pages: write".`
        )
      )
      twirpScope.done()
    })

    it('reports errors with an unexpected 404 during deployment', async () => {
      process.env.GITHUB_SHA = 'build-version'
      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 2 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(404, { message: 'Not found' }, { headers: { 'content-type': 'application/json' } })

      // Create the deployment
      const deployment = new Deployment()
      await expect(deployment.create()).rejects.toEqual(
        new Error(
          `Failed to create deployment (status: 404) with build version ${process.env.GITHUB_SHA}. Ensure GitHub Pages has been enabled: https://github.com/actions/is-awesome/settings/pages`
        )
      )
      twirpScope.done()
    })

    it('reports errors with failed deployments', async () => {
      process.env.GITHUB_SHA = 'invalid-build-version'
      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 2 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(400, { message: 'Bad request' }, { headers: { 'content-type': 'application/json' } })

      // Create the deployment
      const deployment = new Deployment()
      await expect(deployment.create()).rejects.toEqual(
        new Error(
          `Failed to create deployment (status: 400) with build version ${process.env.GITHUB_SHA}. Responded with: Bad request`
        )
      )
      twirpScope.done()
    })

    it('fails if there are multiple artifacts with the same name', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [
              { databaseId: 13, name: 'github-pages', size: 1400 },
              { databaseId: 14, name: 'github-pages', size: 1620 }
            ]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      const deployment = new Deployment()
      await expect(deployment.create(fakeJwt)).rejects.toThrow(
        `Multiple artifacts named "github-pages" were unexpectedly found for this workflow run. Artifact count is 2.`
      )
      twirpScope.done()
    })

    it('fails if there are no artifacts found', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: []
          },
          { headers: { 'content-type': 'application/json' } }
        )

      const deployment = new Deployment()
      await expect(deployment.create(fakeJwt)).rejects.toThrow(
        `No artifacts named "github-pages" were found for this workflow run. Ensure artifacts are uploaded with actions/upload-artifact@v4 or later.`
      )
      twirpScope.done()
    })

    it('fails with error message if list artifact endpoint returns 501', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(501, { msg: 'oh no' }, { headers: { 'content-type': 'application/json' } })

      const deployment = new Deployment()
      await expect(deployment.create(fakeJwt)).rejects.toThrow(
        `Failed to create deployment (status: 501) with build version ${process.env.GITHUB_SHA}. Server error, is githubstatus.com reporting a Pages outage? Please re-run the deployment at a later time.`
      )
      expect(core.error).toHaveBeenNthCalledWith(
        1,
        'Listing artifact metadata failed',
        new Error('Failed to ListArtifacts: Received non-retryable error: Failed request: (501) null: oh no')
      )
      expect(core.error).toHaveBeenNthCalledWith(
        2,
        'Fetching artifact metadata failed. Is githubstatus.com reporting issues with API requests, Pages, or Actions? Please re-run the deployment at a later time.',
        expect.any(Error)
      )

      twirpScope.done()
    })

    it('warns if the artifact size is bigger than maximum', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'
      const artifactSize = ONE_GIGABYTE + 1

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 12, name: 'github-pages', size: artifactSize }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 12 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      const deployment = new Deployment()
      await deployment.create(fakeJwt)

      expect(core.warning).toBeCalledWith(
        `Uploaded artifact size of ${artifactSize} bytes exceeds the allowed size of ${SIZE_LIMIT_DESCRIPTION}. Deployment might fail.`
      )
      expect(core.setFailed).not.toHaveBeenCalled()
      expect(core.info).toHaveBeenLastCalledWith(
        expect.stringMatching(new RegExp(`^Created deployment for ${process.env.GITHUB_SHA}`))
      )
      twirpScope.done()
    })

    it('warns when the timeout is greater than the maximum allowed', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Set timeout to greater than max
      jest.spyOn(core, 'getInput').mockImplementation(param => {
        switch (param) {
          case 'artifact_name':
            return 'github-pages'
          case 'token':
            return process.env.GITHUB_TOKEN
          case 'reporting_interval':
            return 50 // Lower reporting interval to speed up test
          case 'timeout':
            return MAX_TIMEOUT + 1
          default:
            return process.env[`INPUT_${param.toUpperCase()}`] || ''
        }
      })

      const deployment = new Deployment()
      await deployment.create(fakeJwt)

      expect(core.warning).toBeCalledWith(
        `Warning: timeout value is greater than the allowed maximum - timeout set to the maximum of ${MAX_TIMEOUT} milliseconds.`
      )
      twirpScope.done()
    })
  })

  describe('#check', () => {
    afterEach(() => {
      jest.restoreAllMocks()
      delete process.env.INPUT_ERROR_COUNT
      delete process.env.INPUT_REPORTING_INTERVAL
    })

    const mockDeploymentStatus = (status, times = 1) => {
      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
          method: 'GET'
        })
        .reply(200, { status }, { headers: { 'content-type': 'application/json' } })
        .times(times)
    }

    const createPendingDeployment = () => {
      const deployment = new Deployment()
      deployment.deploymentInfo = {
        id: process.env.GITHUB_SHA,
        pending: true
      }
      deployment.startTime = Date.now()
      return deployment
    }

    const runWithoutWaiting = async deployment => {
      const timeoutSpy = jest.spyOn(global, 'setTimeout').mockImplementation(resolve => {
        resolve()
        return 0
      })

      await deployment.check()
      return timeoutSpy
    }

    it('sets output to success when deployment is successful', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
          method: 'GET'
        })
        .reply(200, { status: 'succeed' }, { headers: { 'content-type': 'application/json' } })

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)
      await deployment.check()

      expect(core.setOutput).toBeCalledWith('status', 'succeed')
      expect(core.info).toHaveBeenLastCalledWith('Reported success!')
      twirpScope.done()
    })

    it('fails check when no deployment is found', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'
      const deployment = new Deployment()
      await deployment.check()
      expect(core.setFailed).toBeCalledWith('Deployment not found.')
    })

    it('exits early when deployment is not in progress', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      const deployment = new Deployment()
      await deployment.create(fakeJwt)
      deployment.deploymentInfo.pending = false
      await deployment.check()
      expect(core.setFailed).toBeCalledWith('Unable to get deployment status.')
      twirpScope.done()
    })

    it('enforces max timeout', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
          method: 'GET'
        })
        .reply(200, { status: 'deployment_in_progress' }, { headers: { 'content-type': 'application/json' } })
        .times(2)

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}/cancel`,
          method: 'POST'
        })
        .reply(200, {}, { headers: { 'content-type': 'application/json' } })

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Set timeout to greater than max
      jest.spyOn(core, 'getInput').mockImplementation(param => {
        switch (param) {
          case 'artifact_name':
            return 'github-pages'
          case 'token':
            return process.env.GITHUB_TOKEN
          case 'error_count':
            return 10
          case 'reporting_interval':
            return 50 // Lower the interval for the test
          case 'timeout':
            return MAX_TIMEOUT + 1
          default:
            return process.env[`INPUT_${param.toUpperCase()}`] || ''
        }
      })

      // Jump the "current time" by MAX_TIMEOUT ever time Date.now is called
      const _now = Date.now
      let nowCalls = 0
      const nowSpy = jest.spyOn(Date, 'now').mockImplementation(() => {
        nowCalls++
        return _now() + MAX_TIMEOUT * nowCalls
      })

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)
      await deployment.check()

      nowSpy.mockRestore()

      expect(deployment.timeout).toEqual(MAX_TIMEOUT)
      expect(core.error).toBeCalledWith('Timeout reached, aborting!')
      expect(core.setFailed).toBeCalledWith('Timeout reached, aborting!')
      twirpScope.done()
    })

    it('sets timeout to user timeout if user timeout is less than max timeout', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}/cancel`,
          method: 'POST'
        })
        .reply(200, {}, { headers: { 'content-type': 'application/json' } })

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Set timeout to greater than max
      jest.spyOn(core, 'getInput').mockImplementation(param => {
        switch (param) {
          case 'artifact_name':
            return 'github-pages'
          case 'token':
            return process.env.GITHUB_TOKEN
          case 'error_count':
            return 10
          case 'reporting_interval':
            return 42 // The default of 5000 is too long for the test
          case 'timeout':
            return 42
          default:
            return process.env[`INPUT_${param.toUpperCase()}`] || ''
        }
      })

      const now = Date.now()
      const mockStartTime = now - 42
      jest
        .spyOn(Date, 'now')
        .mockImplementationOnce(() => mockStartTime)
        .mockImplementationOnce(() => now)

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)
      await deployment.check()

      expect(deployment.timeout).toEqual(42)
      expect(core.error).toBeCalledWith('Timeout reached, aborting!')
      expect(core.setFailed).toBeCalledWith('Timeout reached, aborting!')
      twirpScope.done()
    })

    it('sets output to success when timeout is set but not reached', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
          method: 'GET'
        })
        .reply(200, { status: 'succeed' }, { headers: { 'content-type': 'application/json' } })

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Set timeout to greater than max
      jest.spyOn(core, 'getInput').mockImplementation(param => {
        switch (param) {
          case 'artifact_name':
            return 'github-pages'
          case 'token':
            return process.env.GITHUB_TOKEN
          case 'error_count':
            return 10
          case 'reporting_interval':
            return 1 // The default of 5000 is too long for the test
          case 'timeout':
            return 42
          default:
            return process.env[`INPUT_${param.toUpperCase()}`] || ''
        }
      })

      const now = Date.now()
      const mockStartTime = now // No time elapsed
      jest
        .spyOn(Date, 'now')
        .mockImplementationOnce(() => mockStartTime)
        .mockImplementationOnce(() => now)

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)
      await deployment.check()

      expect(deployment.timeout).toEqual(42)
      expect(core.error).not.toBeCalled()
      expect(core.setOutput).toBeCalledWith('status', 'succeed')
      expect(core.info).toHaveBeenLastCalledWith('Reported success!')
      twirpScope.done()
    })

    it('backs off successful non-terminal status checks', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'
      process.env.INPUT_ERROR_COUNT = '10'
      mockDeploymentStatus('deployment_in_progress', 2)
      mockDeploymentStatus('succeed')
      const randomSpy = jest.spyOn(Math, 'random').mockReturnValue(0.5)

      const timeoutSpy = await runWithoutWaiting(createPendingDeployment())

      expect(timeoutSpy).toHaveBeenNthCalledWith(1, expect.any(Function), 50)
      expect(timeoutSpy).toHaveBeenNthCalledWith(2, expect.any(Function), 75)
      expect(timeoutSpy).toHaveBeenNthCalledWith(3, expect.any(Function), 113)

      timeoutSpy.mockRestore()
      randomSpy.mockRestore()
      delete process.env.INPUT_ERROR_COUNT
    })

    it('caps the successful status check backoff', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'
      process.env.INPUT_ERROR_COUNT = '10'
      process.env.INPUT_REPORTING_INTERVAL = '20000'
      mockDeploymentStatus('deployment_in_progress')
      mockDeploymentStatus('succeed')
      const randomSpy = jest.spyOn(Math, 'random').mockReturnValue(0.5)

      const timeoutSpy = await runWithoutWaiting(createPendingDeployment())

      expect(timeoutSpy.mock.calls.map(([, interval]) => interval)).toEqual([20000, MAX_REPORTING_INTERVAL])

      timeoutSpy.mockRestore()
      randomSpy.mockRestore()
    })

    it('does not reduce a configured interval above the backoff cap', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'
      process.env.INPUT_ERROR_COUNT = '10'
      process.env.INPUT_REPORTING_INTERVAL = '45000'
      mockDeploymentStatus('deployment_in_progress')
      mockDeploymentStatus('succeed')
      jest.spyOn(Math, 'random').mockReturnValue(0.5)

      const timeoutSpy = await runWithoutWaiting(createPendingDeployment())

      expect(timeoutSpy.mock.calls.map(([, interval]) => interval)).toEqual([45000, 45000])
    })

    it.each(['not-a-number', '0', '-1'])(
      'uses the default reporting interval for invalid input %s',
      async reportingInterval => {
        process.env.GITHUB_SHA = 'valid-build-version'
        process.env.INPUT_ERROR_COUNT = '10'
        process.env.INPUT_REPORTING_INTERVAL = reportingInterval
        mockDeploymentStatus('succeed')
        jest.spyOn(Math, 'random').mockReturnValue(0.5)

        const timeoutSpy = await runWithoutWaiting(createPendingDeployment())

        expect(timeoutSpy).toHaveBeenCalledWith(expect.any(Function), DEFAULT_REPORTING_INTERVAL)
        expect(core.warning).toHaveBeenCalledWith(
          `Invalid reporting_interval value; using the default of ${DEFAULT_REPORTING_INTERVAL} milliseconds.`
        )
      }
    )

    it('jitters status check intervals by up to twenty percent', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'
      process.env.INPUT_ERROR_COUNT = '10'
      mockDeploymentStatus('deployment_in_progress')
      mockDeploymentStatus('succeed')
      const randomSpy = jest.spyOn(Math, 'random').mockReturnValueOnce(0).mockReturnValueOnce(1)

      const timeoutSpy = await runWithoutWaiting(createPendingDeployment())

      expect(timeoutSpy).toHaveBeenNthCalledWith(1, expect.any(Function), 40)
      expect(timeoutSpy).toHaveBeenNthCalledWith(2, expect.any(Function), 90)

      timeoutSpy.mockRestore()
      randomSpy.mockRestore()
    })

    it('keeps success backoff separate from error backoff', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'
      process.env.INPUT_ERROR_COUNT = '10'
      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
          method: 'GET'
        })
        .reply(500, {}, { headers: { 'content-type': 'application/json' } })
      mockDeploymentStatus('deployment_in_progress')
      mockDeploymentStatus('succeed')
      const randomSpy = jest.spyOn(Math, 'random').mockReturnValue(0.5)

      const timeoutSpy = await runWithoutWaiting(createPendingDeployment())

      expect(timeoutSpy.mock.calls.map(([, interval]) => interval)).toEqual([50, 51, 75])

      timeoutSpy.mockRestore()
      randomSpy.mockRestore()
    })
  })

  describe('#cancel', () => {
    it('can successfully cancel a deployment', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}/cancel`,
          method: 'POST'
        })
        .reply(200, {}, { headers: { 'content-type': 'application/json' } })

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)

      // Cancel it
      await deployment.cancel()

      expect(core.info).toHaveBeenLastCalledWith(`Canceled deployment with ID ${process.env.GITHUB_SHA}`)
      twirpScope.done()
    })

    it('can exit if a pages deployment was not created and none need to be cancelled', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      // Create the deployment
      const deployment = new Deployment()

      // Cancel it
      await deployment.cancel()

      expect(core.debug).toHaveBeenCalledWith('all variables are set')
      expect(core.debug).toHaveBeenCalledWith(`No deployment to cancel`)
    })

    it('catches an error when trying to cancel a deployment', async () => {
      process.env.GITHUB_SHA = 'valid-build-version'

      const twirpScope = nock(process.env.ACTIONS_RESULTS_URL)
        .post(LIST_ARTIFACTS_TWIRP_PATH)
        .reply(
          200,
          {
            artifacts: [{ databaseId: 11, name: 'github-pages', size: 221 }]
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments`,
          method: 'POST',
          body: bodyString => {
            const body = JSON.parse(bodyString)
            const keys = Object.keys(body).sort()
            return (
              keys.length === 3 &&
              keys[0] === 'artifact_id' &&
              keys[1] === 'oidc_token' &&
              keys[2] === 'pages_build_version' &&
              body.artifact_id === 11 &&
              body.oidc_token === fakeJwt &&
              body.pages_build_version === process.env.GITHUB_SHA
            )
          }
        })
        .reply(
          200,
          {
            status_url: `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}`,
            page_url: 'https://actions.github.io/is-awesome'
          },
          { headers: { 'content-type': 'application/json' } }
        )

      mockPool
        .intercept({
          path: `/repos/${process.env.GITHUB_REPOSITORY}/pages/deployments/${process.env.GITHUB_SHA}/cancel`,
          method: 'POST'
        })
        .reply(500, {}, { headers: { 'content-type': 'application/json' } })

      core.getIDToken = jest.fn().mockResolvedValue(fakeJwt)

      // Create the deployment
      const deployment = new Deployment()
      await deployment.create(fakeJwt)

      // Cancel it
      await deployment.cancel()

      expect(core.error).toHaveBeenCalledWith(`Canceling Pages deployment failed`, expect.anything())
      twirpScope.done()
    })
  })
})
