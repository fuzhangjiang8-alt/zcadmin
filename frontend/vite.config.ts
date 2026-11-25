// =============================================================================
// Vite 配置
//
// 关键点：
// 1. 开发环境用 proxy 把 /api 转发到后端，前端与后端同源 —— 这样 Cookie 能正常携带，
//    也绕开了 CORS 预检，开发体验最好。
// 2. 生产环境由 Nginx 或 axum 静态托管，同样保持同源。
// =============================================================================

import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  plugins: [vue()],

  server: {
    port: 5173,
    proxy: {
      // 所有 /api 请求转发到 Rust 后端
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: false,
      },
      // 健康检查也代理，便于本地排查
      '/healthz': 'http://127.0.0.1:8080',
      '/readyz': 'http://127.0.0.1:8080',
    },
  },

  build: {
    // 生产构建输出目录；后续会由 rust-embed 内嵌进单二进制
    outDir: 'dist',
    sourcemap: false,
    // antd 是单一 vendor 块（约 1MB，gzip ≈ 300KB），独立缓存；
    // 阈值略高于它，真正出现异常膨胀时仍会告警。
    chunkSizeWarningLimit: 1100,
    rollupOptions: {
      output: {
        // 第三方库单独成块：它们很少变化，可以被浏览器长期缓存。
        // 业务代码改动时不需要重新下载 1MB 的组件库。
        manualChunks: {
          vue: ['vue', 'vue-router', 'vue-i18n', '@tanstack/vue-query'],
          antd: ['ant-design-vue'],
        },
      },
    },
  },
});
