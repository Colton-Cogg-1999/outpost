import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri expects a fixed port and handles the window itself.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: '127.0.0.1', watch: { ignored: ['**/src-tauri/**'] } },
  build: { target: 'es2022', sourcemap: false, chunkSizeWarningLimit: 1200 },
  test: { environment: 'jsdom', include: ['src/**/*.test.{ts,tsx}'], globals: false },
});
