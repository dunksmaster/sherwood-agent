import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Strict CSP for the built app only — dev needs inline scripts for HMR.
const CSP =
  "default-src 'self'; connect-src 'self'; img-src 'self' data:; " +
  "style-src 'self' 'unsafe-inline'; script-src 'self'; base-uri 'none'; " +
  "form-action 'none'; frame-ancestors 'none'";

export default defineConfig({
  plugins: [
    react(),
    {
      name: "csp-on-build",
      apply: "build",
      transformIndexHtml(html) {
        return html.replace(
          "</head>",
          `  <meta http-equiv="Content-Security-Policy" content="${CSP}" />\n</head>`,
        );
      },
    },
  ],
  server: {
    port: 5173,
    // Dev: proxy the API so the browser talks same-origin and no CORS config is
    // needed. In production the server serves the built bundle from its own
    // origin.
    proxy: {
      "/v1": {
        target: "http://127.0.0.1:8787",
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
