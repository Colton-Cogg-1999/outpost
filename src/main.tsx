import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/silkscreen/400.css';
import '@fontsource/vt323/400.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import './styles.css';
import App from './App';
import { runSelftest } from './dev/selftest';
import { reportErrors } from './lib/ipc';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

reportErrors();
runSelftest();
