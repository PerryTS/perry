const process = require('process')
const cp = require('child_process')
const path = require('path')

describe('with all environment variables set', () => {
  beforeEach(() => {
    process.env.GITHUB_RUN_ID = '123'
    process.env.GITHUB_REPOSITORY = 'actions/is-awesome'
    process.env.GITHUB_TOKEN = 'gha-token'
    process.env.GITHUB_SHA = '123abc'
    process.env.GITHUB_ACTOR = 'monalisa'
    process.env.GITHUB_ACTION = '__monalisa/octocat'
    process.env.GITHUB_ACTION_PATH = 'something'
  })

  it('executes cleanly', done => {
    const ip = path.join(__dirname, '../index.js')
    cp.exec(`node ${ip}`, { env: process.env }, (err, stdout) => {
      expect(stdout).toMatch(/::debug::all variables are set/)
      done()
    })
  })
})

describe('with variables missing', () => {
  it('execution fails if there are missing variables', done => {
    delete process.env.GITHUB_RUN_ID
    const ip = path.join(__dirname, '../index.js')
    cp.exec(`node ${ip}`, { env: process.env }, (err, stdout) => {
      expect(stdout).toBe('')
      expect(err).toBeTruthy()
      expect(err.code).toBe(1)
      done()
    })
  })
})
