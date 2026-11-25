<template>
  <!-- 根组件只做路由出口 + 全局中文 locale，不放任何业务逻辑 -->
  <a-config-provider :locale="zhCN">
    <router-view />
  </a-config-provider>
</template>

<script setup lang="ts">
// Ant Design Vue 默认 locale 是英文，分页「Go to / Page」、日期选择器月份、
// 空状态「No Data」等都会显示英文。这里统一注入简体中文，覆盖全部组件。
import zhCN from 'ant-design-vue/es/locale/zh_CN';
</script>
