import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

const developmentTrustedTypesCompatibility = {
  name: 'development-trusted-types-compatibility',
  transformIndexHtml(html: string) {
    return html.replace(
      "; require-trusted-types-for 'script'; trusted-types sprout",
      '',
    )
  },
}

export default defineConfig(({ command }) => {
  const proxyTarget = process.env.SPROUT_DEV_API_PROXY_TARGET
  return {
    plugins: [
      react(),
      ...(command === 'serve' ? [developmentTrustedTypesCompatibility] : []),
    ],
    build: {
      target: 'es2022',
      sourcemap: false,
    },
    server: proxyTarget
      ? {
          proxy: {
            '/v1': { target: proxyTarget },
            '/health': { target: proxyTarget },
          },
        }
      : undefined,
  }
})
