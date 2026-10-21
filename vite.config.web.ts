import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Web 版构建：浏览器中运行 React 界面，通过 /api/* 与 Rust 服务通信。
// 桌面构建仍使用 vite.config.ts，互不影响。
const webFavicon = {
  name: "cc-switch-web-favicon",
  transformIndexHtml(html: string) {
    return {
      html,
      tags: [
        {
          tag: "link",
          attrs: {
            rel: "icon",
            type: "image/svg+xml",
            href: "/favicon.svg",
          },
          injectTo: "head-prepend" as const,
        },
        {
          tag: "link",
          attrs: {
            rel: "icon",
            type: "image/x-icon",
            href: "/favicon.ico",
          },
          injectTo: "head-prepend" as const,
        },
        {
          tag: "link",
          attrs: {
            rel: "apple-touch-icon",
            href: "/apple-touch-icon.png",
          },
          injectTo: "head-prepend" as const,
        },
      ],
    };
  },
};

export default defineConfig({
  root: "src",
  publicDir: path.resolve(__dirname, "public-web"),
  plugins: [react(), webFavicon],
  base: "/",
  build: {
    outDir: "../dist-web",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, "src/index.html"),
      },
    },
  },
  server: {
    port: 3001,
    strictPort: true,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8787",
        changeOrigin: false,
      },
    },
  },
  resolve: {
    alias: [
      {
        find: "@tauri-apps/api/core",
        replacement: path.resolve(__dirname, "./src/web/shims/core.ts"),
      },
      {
        find: "@tauri-apps/api/event",
        replacement: path.resolve(__dirname, "./src/web/shims/event.ts"),
      },
      {
        find: "@tauri-apps/api/app",
        replacement: path.resolve(__dirname, "./src/web/shims/app.ts"),
      },
      {
        find: "@tauri-apps/api/window",
        replacement: path.resolve(__dirname, "./src/web/shims/window.ts"),
      },
      {
        find: "@tauri-apps/api/path",
        replacement: path.resolve(__dirname, "./src/web/shims/path.ts"),
      },
      {
        find: "@tauri-apps/plugin-dialog",
        replacement: path.resolve(__dirname, "./src/web/shims/dialog.ts"),
      },
      {
        find: "@tauri-apps/plugin-log",
        replacement: path.resolve(__dirname, "./src/web/shims/log.ts"),
      },
      {
        find: "@tauri-apps/plugin-process",
        replacement: path.resolve(__dirname, "./src/web/shims/process.ts"),
      },
      {
        find: "@tauri-apps/plugin-updater",
        replacement: path.resolve(__dirname, "./src/web/shims/updater.ts"),
      },
      {
        find: "@",
        replacement: path.resolve(__dirname, "./src"),
      },
    ],
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
});
