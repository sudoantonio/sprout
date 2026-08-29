import './theme-init'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { registerServiceWorker } from './pwa'

const userAgent = navigator.userAgent
const isSafari =
  /Safari/i.test(userAgent) &&
  !/(Chrome|Chromium|CriOS|Edg|OPR|FxiOS)/i.test(userAgent)

if (isSafari) {
  document.documentElement.dataset.browser = 'safari'
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

void registerServiceWorker()
