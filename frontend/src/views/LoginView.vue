<template>
  <div class="login-page">
    <div class="login-shell">
      <!-- 品牌展示区 -->
      <div class="brand-pane">
        <div class="brand-badge">Z</div>
        <h1 class="brand-title">{{ t('common.appName') }}</h1>
        <p class="brand-desc">{{ t('login.subtitle') }}</p>
        <ul class="brand-points">
          <li v-for="point in brandPoints" :key="point">{{ point }}</li>
        </ul>
      </div>

      <!-- 登录表单 -->
      <div class="form-pane">
        <a-card class="login-card" :bordered="false">
          <h2 class="form-title">{{ t('common.appName') }}</h2>
          <p class="form-subtitle">{{ t('login.subtitle') }}</p>

          <a-form :model="form" layout="vertical" @finish="onSubmit">
            <a-form-item
              :label="t('login.username')"
              name="username"
              :rules="[{ required: true, message: t('login.usernameRequired') }]"
            >
              <a-input v-model:value="form.username" size="large" placeholder="admin" autocomplete="username" />
            </a-form-item>

            <a-form-item
              :label="t('login.password')"
              name="password"
              :rules="[{ required: true, message: t('login.passwordRequired') }]"
            >
              <a-input-password
                v-model:value="form.password"
                size="large"
                placeholder="admin123456"
                autocomplete="current-password"
              />
            </a-form-item>

            <a-form-item :label="t('login.tenant')" name="tenant">
              <a-input v-model:value="form.tenant" size="large" placeholder="default" />
            </a-form-item>

            <a-alert v-if="errorMessage" type="error" :message="errorMessage" show-icon class="error" />

            <a-button type="primary" html-type="submit" size="large" block :loading="loading" class="submit-btn">
              {{ t('login.submit') }}
            </a-button>
          </a-form>
        </a-card>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 登录页（左右分栏：品牌区 + 表单）
//
// 说明：
// - 登录成功后后端会写 HttpOnly Cookie，前端不保存任何令牌；
// - 错误信息来自后端 problem+json 的 detail 字段，前端不重复维护文案。
// =============================================================================

import { computed, reactive, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../api/client';
import { clearSession } from '../stores/session';

interface LoginResponse {
  user: { userId: string; username: string; permissions: string[] };
  expiresAt: string;
}

const { t } = useI18n();
const router = useRouter();
const route = useRoute();

const form = reactive({
  username: '',
  password: '',
  tenant: typeof route.query.tenant === 'string' ? route.query.tenant : '',
});
const loading = ref(false);
const errorMessage = ref('');

const brandPoints = computed(() => [
  t('login.point1'),
  t('login.point2'),
  t('login.point3'),
]);

/** 登录成功后废弃旧身份和查询缓存，再由路由守卫重新确认当前账号。 */
async function onSubmit(): Promise<void> {
  loading.value = true;
  errorMessage.value = '';

  try {
    await http.post<LoginResponse>('/api/auth/login', {
      username: form.username,
      password: form.password,
      tenant: form.tenant.trim() || null,
    });

    clearSession();
    // 登录成功：回到守卫记录的原目标（默认首页）
    const redirect = typeof route.query.redirect === 'string' ? route.query.redirect : '/';
    await router.replace(redirect);
  } catch (error) {
    // 统一错误展示：后端已经给出人类可读文案
    errorMessage.value =
      error instanceof ApiError ? error.message : t('login.loginFailed');
  } finally {
    loading.value = false;
  }
}
</script>

<style scoped>
.login-page {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background:
    radial-gradient(900px 480px at 15% 10%, rgb(99 102 241 / 18%), transparent 60%),
    radial-gradient(700px 420px at 85% 90%, rgb(14 165 233 / 14%), transparent 55%),
    #0f172a;
}

/* ---- 分栏外壳 ---- */
.login-shell {
  display: flex;
  width: 900px;
  max-width: 100%;
  border-radius: 20px;
  overflow: hidden;
  background: #fff;
  box-shadow: 0 32px 80px rgb(2 6 23 / 45%);
  animation: shell-in 0.5s ease;
}
@keyframes shell-in {
  from { opacity: 0; transform: translateY(16px) scale(0.98); }
  to { opacity: 1; transform: translateY(0) scale(1); }
}

/* ---- 左侧品牌区 ---- */
.brand-pane {
  flex: 1;
  padding: 56px 44px;
  color: #e2e8f0;
  background:
    radial-gradient(400px 260px at 80% 0%, rgb(99 102 241 / 40%), transparent 60%),
    linear-gradient(160deg, #1e1b4b 0%, #312e81 55%, #0c4a6e 100%);
}
.brand-badge {
  width: 52px;
  height: 52px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 14px;
  font-size: 26px;
  font-weight: 800;
  color: #fff;
  background: linear-gradient(135deg, #818cf8, #22d3ee);
  box-shadow: 0 10px 28px rgb(99 102 241 / 50%);
  margin-bottom: 24px;
}
.brand-title {
  margin: 0 0 8px;
  font-size: 28px;
  font-weight: 700;
  color: #fff;
}
.brand-desc {
  margin: 0 0 32px;
  font-size: 14px;
  color: #c7d2fe;
  line-height: 1.7;
}
.brand-points {
  margin: 0;
  padding: 0;
  list-style: none;
}
.brand-points li {
  position: relative;
  padding-left: 20px;
  margin-bottom: 14px;
  font-size: 13px;
  color: #cbd5e1;
  line-height: 1.6;
}
.brand-points li::before {
  content: '';
  position: absolute;
  left: 0;
  top: 8px;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: linear-gradient(135deg, #818cf8, #22d3ee);
}

/* ---- 右侧表单区 ---- */
.form-pane {
  flex: 1;
  display: flex;
  align-items: center;
  padding: 40px 44px;
}
.login-card {
  width: 100%;
  box-shadow: none;
}
.form-title {
  margin: 0;
  font-size: 22px;
  font-weight: 700;
  color: #0f172a;
}
.form-subtitle {
  margin: 4px 0 24px;
  color: #64748b;
  font-size: 13px;
}
.submit-btn {
  margin-top: 8px;
  height: 44px;
  font-weight: 600;
  border-radius: 10px;
}
.error {
  margin-bottom: 16px;
}

@media (max-width: 768px) {
  .login-shell { flex-direction: column; width: 420px; }
  .brand-pane { padding: 32px 28px; }
  .form-pane { padding: 28px; }
}
</style>
