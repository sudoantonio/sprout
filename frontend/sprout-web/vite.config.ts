import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig(() => {
  const proxyTarget = process.env.SPROUT_DEV_API_PROXY_TARGET
  return {
    plugins: [react()],
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
