<!--
  =============================================================================
  403 页面

  职责：路由级权限不通过时的落点，避免用户直接输入 URL 看到「空壳页面」。
  边界：只做展示与返回，不发起任何业务请求。
  关键约束：文案与按钮必须使用已注册的 AntD 组件（见 main.ts 的按需注册清单）。
  =============================================================================
-->
<template>
  <div class="forbidden">
    <a-card :bordered="false" class="forbidden-card">
      <div class="code">403</div>
      <h2>{{ t('forbidden.title') }}</h2>
      <p class="hint">{{ t('forbidden.hint') }}</p>
      <a-space>
        <a-button type="primary" @click="goHome">{{ t('forbidden.goHome') }}</a-button>
        <a-button @click="goBack">{{ t('forbidden.goBack') }}</a-button>
      </a-space>
    </a-card>
  </div>
</template>

<script setup lang="ts">
import { useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';

const { t } = useI18n();
const router = useRouter();

function goHome(): void {
  void router.push('/');
}

function goBack(): void {
  if (window.history.length > 1) {
    router.back();
  } else {
    void router.push('/');
  }
}
</script>

<style scoped>
.forbidden {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 60vh;
}

.forbidden-card {
  max-width: 480px;
  text-align: center;
}

.code {
  font-size: 56px;
  font-weight: 700;
  color: #1677ff;
  line-height: 1.2;
}

.hint {
  color: #64748b;
  margin: 8px 0 20px;
}
</style>
