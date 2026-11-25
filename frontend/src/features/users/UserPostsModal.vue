<template>
  <a-modal
    :open="open"
    :title="t('users.postsTitle', { name: user?.username ?? '' })"
    :confirm-loading="saving"
    :ok-text="t('users.save')"
    :cancel-text="t('users.cancel')"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-spin :spinning="loading">
      <a-checkbox-group v-model:value="checkedIds" :options="options" class="post-group" />
      <a-empty v-if="options.length === 0" :description="t('users.noPosts')" />
    </a-spin>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 用户岗位分配
//
// 保存成功后 emit('saved')，由父页面刷新列表（列表里展示岗位名）。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import type { PostVo, UserVo } from './types';

const props = defineProps<{ open: boolean; user: UserVo | null }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const loading = ref(false);
const saving = ref(false);
const posts = ref<PostVo[]>([]);
const checkedIds = ref<string[]>([]);

const options = computed(() => posts.value.map((p) => ({ label: p.name, value: p.id })));

async function load(): Promise<void> {
  if (!props.user) return;
  loading.value = true;
  try {
    const [all, mine] = await Promise.all([
      http.get<{ data: PostVo[] }>('/api/system/posts?page=1&size=200'),
      http.get<string[]>(`/api/system/users/${props.user.id}/posts`),
    ]);
    posts.value = all.data;
    checkedIds.value = mine;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.postsLoadFailed'));
  } finally {
    loading.value = false;
  }
}

async function save(): Promise<void> {
  if (!props.user) return;
  saving.value = true;
  try {
    await http.put<string[]>(`/api/system/users/${props.user.id}/posts`, {
      postIds: checkedIds.value,
    });
    message.success(t('users.postsSaved'));
    emit('update:open', false);
    emit('saved');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.saveFailedShort'));
  } finally {
    saving.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void load();
  },
);
</script>

<style scoped>
.post-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
</style>
