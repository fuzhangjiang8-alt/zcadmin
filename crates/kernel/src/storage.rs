//! 对象存储抽象（M3：本地磁盘实现）。
//!
//! # 为什么先做本地实现
//!
//! 私有化交付场景里，客户往往没有 S3/MinIO；本地磁盘 + 定期备份是最小可用方案。
//! 接口保持与对象存储一致（`save / load / remove` + 由服务端生成 key），
//! 后续接 S3 只需要替换本文件的实现，业务代码不动。
//!
//! # 安全约束
//!
//! 存储 key **必须由服务端生成**（`{tenant_id}/{file_id}`），
//! 且 `resolve` 会拒绝绝对路径与 `..`，防止路径穿越写到任意位置。

use anyhow::{bail, Context};
use std::path::{Path, PathBuf};

/// 本地磁盘存储。
#[derive(Debug, Clone)]
pub struct LocalStorage {
    /// 根目录。
    root: PathBuf,
}

impl LocalStorage {
    /// 创建存储实例（目录不存在时自动创建）。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        if let Err(e) = std::fs::create_dir_all(&root) {
            // 这里不直接失败：首次写入时会给出更明确的错误
            tracing::warn!(dir = %root.display(), error = %e, "创建存储目录失败");
        }
        Self { root }
    }

    /// 把相对 key 解析为绝对路径，并做安全校验。
    fn resolve(&self, key: &str) -> anyhow::Result<PathBuf> {
        let path = Path::new(key);
        if key.is_empty() || path.is_absolute() || path.has_root() || key.contains("..") {
            bail!("非法的存储 key: {key}");
        }
        Ok(self.root.join(path))
    }

    /// 写入文件（覆盖同 key）。
    pub async fn save(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }
        tokio::fs::write(&path, bytes)
            .await
            .with_context(|| format!("写入文件失败: {}", path.display()))?;
        Ok(())
    }

    /// 读取文件全部内容。
    pub async fn load(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let path = self.resolve(key)?;
        tokio::fs::read(&path)
            .await
            .with_context(|| format!("读取文件失败: {}", path.display()))
    }

    /// 删除文件（不存在视为成功，保证幂等）。
    pub async fn remove(&self, key: &str) -> anyhow::Result<()> {
        let path = self.resolve(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("删除文件失败: {}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("zcadmin-test-{}", uuid::Uuid::new_v4()))
    }

    /// 路径穿越必须被拒绝（这是文件模块最重要的安全断言）。
    #[test]
    fn rejects_path_traversal() {
        let s = LocalStorage::new(temp_root());
        assert!(s.resolve("../etc/passwd").is_err());
        assert!(s.resolve("/etc/passwd").is_err());
        assert!(s.resolve("a/../../b").is_err());
        assert!(s.resolve("tenant/file").is_ok());
    }

    /// 写入后能读回，删除后读不到。
    #[tokio::test]
    async fn save_load_remove_roundtrip() {
        let root = temp_root();
        let s = LocalStorage::new(&root);
        s.save("t1/f1", b"hello").await.expect("save");
        assert_eq!(s.load("t1/f1").await.expect("load"), b"hello");
        s.remove("t1/f1").await.expect("remove");
        assert!(s.load("t1/f1").await.is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
