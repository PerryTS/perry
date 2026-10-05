const { convertErrorToAnnotationProperties } = require('./error-utils')

describe('error-utils', () => {
  describe('convertErrorToAnnotationProperties', () => {
    it('throws a TypeError if the first argument is not an Error instance', () => {
      expect(() => convertErrorToAnnotationProperties('not an Error')).toThrow(
        TypeError,
        'error must be an instance of Error'
      )
    })

    it('throws an Error if the first argument is an Error instance without a parseable stack', () => {
      const error = new Error('Test error')
      error.stack = ''
      expect(() => convertErrorToAnnotationProperties(error)).toThrow(Error, 'Error stack is empty or unparseable')
    })

    it('returns an AnnotationProperties-compatible object', () => {
      const result = convertErrorToAnnotationProperties(new TypeError('Test error'))
      expect(result).toEqual({
        title: 'TypeError',
        file: __filename,
        startLine: expect.any(Number),
        startColumn: expect.any(Number)
      })
    })

    it('returns an AnnotationProperties-compatible object with a custom title', () => {
      const result = convertErrorToAnnotationProperties(new TypeError('Test error'), 'custom title')
      expect(result).toEqual({
        title: 'custom title',
        file: __filename,
        startLine: expect.any(Number),
        startColumn: expect.any(Number)
      })
    })
  })
})
