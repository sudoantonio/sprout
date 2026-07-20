import { defineConfig, devices } from '@playwright/test'

const externalBaseUrl = process.env.PLAYWRIGHT_BASE_URL

export default defineConfig({
  testDir: './tests/e2e',
  testIgnore: '**/pwa-journey.spec.ts',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  reporter: 'list',
  use: {
    baseURL: externalBaseUrl ?? 'http://localhost:4173',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        launchOptions: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE
          ? {
              executablePath:
                process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
            }
          : undefined,
      },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
    {
      name: 'webkit',
      use: {
        browserName: 'webkit',
        viewport: { width: 1280, height: 720 },
      },
    },
  ],
  webServer: externalBaseUrl
    ? undefined
    : {
        command: 'npm run dev -- --host 0.0.0.0 --port 4173',
        url: 'http://localhost:4173',
        reuseExistingServer: !process.env.CI,
      },
})
