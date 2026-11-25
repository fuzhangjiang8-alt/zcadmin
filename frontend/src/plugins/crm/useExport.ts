// =============================================================================
// 业务导出 composable
//
// 创建异步导出任务 → 轮询状态 → 成功后打开下载链接。
// 与审计导出同一套后端任务机制，只是 kind 不同（由插件注册）。
// =============================================================================

import { ref } from 'vue';
import { message } from 'ant-design-vue';
import { http } from '../../api/client';

interface ExportTask {
  id: string;
  status: string;
  error: string | null;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

export function useExport(kind: string) {
  const exporting = ref(false);

  async function exportNow(): Promise<void> {
    exporting.value = true;
    try {
      const task = await http.post<ExportTask>('/api/system/export-tasks', { kind });
      for (let i = 0; i < 60; i += 1) {
        await new Promise((resolve) => setTimeout(resolve, 500));
        const page = await http.get<PageResult<ExportTask>>(
          '/api/system/export-tasks?page=1&size=10',
        );
        const current = page.data.find((t) => t.id === task.id);
        if (current?.status === 'success') {
          window.open(`/api/system/export-tasks/${task.id}/download`, '_blank');
          message.success('导出完成');
          return;
        }
        if (current?.status === 'failed') {
          message.error(current.error ?? '导出失败');
          return;
        }
      }
      message.warning('导出任务仍在处理中，请稍后到「导出任务」页面查看');
    } catch (error) {
      message.error(error instanceof Error ? error.message : '导出失败');
    } finally {
      exporting.value = false;
    }
  }

  return { exporting, exportNow };
}
