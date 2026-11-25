// =============================================================================
// 业务导入 composable
//
// 上传 xlsx 到 /api/system/imports/{kind}，同步拿到结果。
// 与导出一样，kind 由插件注册，前端只是调用方。
// =============================================================================

import { ref } from 'vue';
import { Modal, message } from 'ant-design-vue';

interface ImportResult {
  success: number;
  failed: number;
  errors: string[];
}

export function useImport(kind: string) {
  const importing = ref(false);

  async function importFile(file: File): Promise<boolean> {
    importing.value = true;
    try {
      const form = new FormData();
      form.append('file', file);
      const resp = await fetch(`/api/system/imports/${kind}`, {
        method: 'POST',
        body: form,
        credentials: 'include',
      });
      const payload = await resp.json().catch(() => null);
      if (!resp.ok) {
        throw new Error(payload?.detail ?? payload?.title ?? '导入失败');
      }
      const result = payload.data as ImportResult;
      message.success(`导入完成：成功 ${result.success} 行，失败 ${result.failed} 行`);
      if (result.errors.length > 0) {
        Modal.warning({
          title: '部分行导入失败',
          content: result.errors.join('\n'),
          width: 640,
        });
      }
      return true;
    } catch (error) {
      message.error(error instanceof Error ? error.message : '导入失败');
      return false;
    } finally {
      importing.value = false;
    }
  }

  return { importing, importFile };
}
