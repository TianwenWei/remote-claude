import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// https://vitejs.dev/config/
export default defineConfig(() => ({
  plugins: [
    react(),
    tailwindcss(),
    // TEMP DEBUG: beacon endpoint for composer trigger diagnosis. Remove after.
    {
      name: "dbg-beacon",
      configureServer(server) {
        server.middlewares.use("/__dbg", (req, res) => {
          void import("node:fs").then((fs) =>
            fs.appendFileSync("/tmp/composer-dbg.log", new Date().toISOString() + " " + (req.url ?? "") + "\n"),
          );
          res.statusCode = 204;
          res.end();
        });
      },
    },
  ],
  resolve: {
    alias: [
      // xterm 6's pre-minified ESM is miscompiled by esbuild on DECRQM
      // (requestMode). Its CJS build avoids the broken enum initializer.
      // Keep this exact so @xterm/xterm/css/xterm.css resolves normally.
      // https://github.com/xtermjs/xterm.js/issues/5800
      {
        find: /^@xterm\/xterm$/,
        replacement: path.resolve(__dirname, "node_modules/@xterm/xterm/lib/xterm.js"),
      },
      { find: "@", replacement: path.resolve(__dirname, "./src") },
    ],
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    fs: {
      // Sibling plugin repos (e.g. ../ccgui-plugin/usage-stats) are served in
      // dev so external plugins can be exercised without installing.
      allow: [".."],
    },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
