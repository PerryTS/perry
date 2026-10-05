// Default Pages configuration for SvelteKit
import adapter from '@sveltejs/adapter-auto'

export default {
  kit: {
    prerender: { origin: 'https://configure-pages.github.io' },
    paths: { base: '/docs' },
    adapter: adapter()
  }
}
