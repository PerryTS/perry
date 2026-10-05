const core = require('@actions/core')
const apiClient = require('./api-client')
const { RequestError } = require('@octokit/request-error')

const mockGetPages = jest.fn()
const mockCreatePagesSite = jest.fn()

const generateRequestError = statusCode => {
  const fakeRequest = { headers: {}, url: '/' }
  const fakeResponse = { status: statusCode }
  let message = 'Oops'
  if (statusCode === 404) {
    message = 'Not Found'
  }
  if (statusCode === 409) {
    message = 'Too Busy'
  }
  const error = new RequestError(message, statusCode, { request: fakeRequest, response: fakeResponse })
  return error
}

jest.mock('@actions/github', () => ({
  context: {
    repo: {
      owner: 'actions',
      repo: 'is-awesome'
    }
  },
  getOctokit: () => ({
    rest: {
      repos: {
        getPages: mockGetPages,
        createPagesSite: mockCreatePagesSite
      }
    }
  })
}))

describe('apiClient', () => {
  const GITHUB_TOKEN = 'gha-token'
  const PAGE_OBJECT = { html_url: 'https://actions.github.io/is-awesome/' }

  beforeEach(() => {
    jest.restoreAllMocks()
    jest.clearAllMocks()
    jest.resetAllMocks()

    // Mock error/warning/info/debug
    jest.spyOn(core, 'error').mockImplementation(jest.fn())
    jest.spyOn(core, 'warning').mockImplementation(jest.fn())
    jest.spyOn(core, 'info').mockImplementation(jest.fn())
    jest.spyOn(core, 'debug').mockImplementation(jest.fn())
  })

  describe('enablePagesSite', () => {
    it('makes a request to create a page', async () => {
      mockCreatePagesSite.mockImplementationOnce(() => Promise.resolve({ status: 201, data: PAGE_OBJECT }))

      const result = await apiClient.enablePagesSite({
        githubToken: GITHUB_TOKEN
      })
      expect(result).toEqual(PAGE_OBJECT)
    })

    it('handles a 409 response when the page already exists', async () => {
      mockCreatePagesSite.mockImplementationOnce(() => Promise.reject(generateRequestError(409)))

      // Simply assert that no error is raised
      const result = await apiClient.enablePagesSite({
        githubToken: GITHUB_TOKEN
      })

      expect(result).toBe(null)
    })

    it('re-raises errors on failure status codes', async () => {
      mockCreatePagesSite.mockImplementationOnce(() => Promise.reject(generateRequestError(404)))

      let erred = false
      try {
        await apiClient.enablePagesSite({
          githubToken: GITHUB_TOKEN
        })
      } catch (error) {
        erred = true
        expect(error.response.status).toEqual(404)
      }
      expect(erred).toBe(true)
    })
  })

  describe('getPagesSite', () => {
    it('makes a request to get a page', async () => {
      const PAGE_OBJECT = { html_url: 'https://actions.github.io/is-awesome/' }
      mockGetPages.mockImplementationOnce(() => Promise.resolve({ status: 200, data: PAGE_OBJECT }))

      const result = await apiClient.getPagesSite({
        githubToken: GITHUB_TOKEN
      })
      expect(result).toEqual(PAGE_OBJECT)
    })

    it('re-raises errors on failure status codes', async () => {
      mockGetPages.mockImplementationOnce(() => Promise.reject(generateRequestError(404)))

      let erred = false
      try {
        await apiClient.getPagesSite({
          githubToken: GITHUB_TOKEN
        })
      } catch (error) {
        erred = true
        expect(error.response.status).toEqual(404)
      }
      expect(erred).toBe(true)
    })
  })

  describe('findOrCreatePagesSite', () => {
    it('does not make a request to create a page if it already exists', async () => {
      const PAGE_OBJECT = { html_url: 'https://actions.github.io/is-awesome/' }
      mockGetPages.mockImplementationOnce(() => Promise.resolve({ status: 200, data: PAGE_OBJECT }))
      mockCreatePagesSite.mockImplementationOnce(() => Promise.reject(generateRequestError(404)))

      const result = await apiClient.findOrCreatePagesSite({
        githubToken: GITHUB_TOKEN
      })
      expect(result).toEqual(PAGE_OBJECT)
      expect(mockGetPages).toHaveBeenCalledTimes(1)
      expect(mockCreatePagesSite).toHaveBeenCalledTimes(0)
    })

    it('makes request to create a page by default if it does not exist', async () => {
      const PAGE_OBJECT = { html_url: 'https://actions.github.io/is-awesome/' }
      mockGetPages.mockImplementationOnce(() => Promise.reject(generateRequestError(404)))
      mockCreatePagesSite.mockImplementationOnce(() => Promise.resolve({ status: 201, data: PAGE_OBJECT }))

      const result = await apiClient.findOrCreatePagesSite({
        githubToken: GITHUB_TOKEN
      })
      expect(result).toEqual(PAGE_OBJECT)
      expect(mockGetPages).toHaveBeenCalledTimes(1)
      expect(mockCreatePagesSite).toHaveBeenCalledTimes(1)
    })

    it('makes a request to create a page when explicitly enabled if it does not exist', async () => {
      const PAGE_OBJECT = { html_url: 'https://actions.github.io/is-awesome/' }
      mockGetPages.mockImplementationOnce(() => Promise.reject(generateRequestError(404)))
      mockCreatePagesSite.mockImplementationOnce(() => Promise.resolve({ status: 201, data: PAGE_OBJECT }))

      const result = await apiClient.findOrCreatePagesSite({
        githubToken: GITHUB_TOKEN,
        enablement: true
      })
      expect(result).toEqual(PAGE_OBJECT)
      expect(mockGetPages).toHaveBeenCalledTimes(1)
      expect(mockCreatePagesSite).toHaveBeenCalledTimes(1)
    })

    it('does not make a request to create a page when explicitly disabled even if it does not exist', async () => {
      mockGetPages.mockImplementationOnce(() => Promise.reject(generateRequestError(404)))
      mockCreatePagesSite.mockImplementationOnce(() => Promise.reject(generateRequestError(500))) // just so they both aren't 404

      let erred = false
      try {
        await apiClient.findOrCreatePagesSite({
          githubToken: GITHUB_TOKEN,
          enablement: false
        })
      } catch (error) {
        erred = true
        // re-raised error
        expect(error.response.status).toEqual(404)
      }
      expect(erred).toBe(true)
      expect(mockGetPages).toHaveBeenCalledTimes(1)
      expect(mockCreatePagesSite).toHaveBeenCalledTimes(0)
    })

    it('does not make a second request to get page if create fails for reason other than existence', async () => {
      mockGetPages.mockImplementationOnce(() => Promise.reject(generateRequestError(404)))
      mockCreatePagesSite.mockImplementationOnce(() => Promise.reject(generateRequestError(500))) // just so they both aren't 404

      let erred = false
      try {
        await apiClient.findOrCreatePagesSite({
          githubToken: GITHUB_TOKEN
        })
      } catch (error) {
        erred = true
        // re-raised error
        expect(error.response.status).toEqual(500)
      }
      expect(erred).toBe(true)
      expect(mockGetPages).toHaveBeenCalledTimes(1)
      expect(mockCreatePagesSite).toHaveBeenCalledTimes(1)
    })

    it('makes second request to get page if create fails because of existence', async () => {
      const PAGE_OBJECT = { html_url: 'https://actions.github.io/is-awesome/' }
      mockGetPages
        .mockImplementationOnce(() => Promise.reject(generateRequestError(404)))
        .mockImplementationOnce(() => Promise.resolve({ status: 200, data: PAGE_OBJECT }))
      mockCreatePagesSite.mockImplementationOnce(() => Promise.reject(generateRequestError(409)))

      const result = await apiClient.findOrCreatePagesSite({
        githubToken: GITHUB_TOKEN
      })
      expect(result).toEqual(PAGE_OBJECT)
      expect(mockGetPages).toHaveBeenCalledTimes(2)
      expect(mockCreatePagesSite).toHaveBeenCalledTimes(1)
    })
  })
})
