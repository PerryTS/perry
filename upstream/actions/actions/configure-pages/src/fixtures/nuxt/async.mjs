const getAllDynamicRoute = async function () {
  const routes = await (async () => {
    return ['/posts/hello-world', '/posts/hello-again']
  })()
  return routes
}

export default {
  mode: 'universal',
  generate: {
    async routes() {
      return getAllDynamicRoute()
    }
  }
}
