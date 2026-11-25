//! 有界的本实例会话缓存；代次检查阻止注销前开始的数据库查询在注销后回填。
//! 不负责跨进程失效广播；缓存清理只影响命中率，不改变数据库会话状态。

use super::{Session, SESSION_CACHE_TTL};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

const MAX_ENTRIES: usize = 10_000;

#[derive(Default)]
struct Entries {
    generation: u64,
    values: HashMap<String, (Session, Instant)>,
}

/// 同一把锁保护代次和缓存项，避免清空后旧结果重新进入缓存。
#[derive(Default)]
pub(super) struct SessionCache {
    entries: RwLock<Entries>,
}

impl SessionCache {
    /// 返回查询开始时的代次；过期会话不能因尚未执行后台清理而继续命中。
    pub(super) fn get(&self, key: &str) -> (u64, Option<Session>) {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let session = entries.values.get(key).and_then(|(session, cached_at)| {
            (cached_at.elapsed() < SESSION_CACHE_TTL && session.expires_at > Utc::now())
                .then(|| session.clone())
        });
        (entries.generation, session)
    }

    /// 仅回填同代次的有效会话；满额时淘汰一项，避免整表扫描阻塞请求线程或清空引发回源尖峰。
    pub(super) fn insert(&self, key: String, session: Session, generation: u64) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if entries.generation != generation || session.expires_at <= Utc::now() {
            return;
        }
        if entries.values.len() >= MAX_ENTRIES && !entries.values.contains_key(&key) {
            if let Some(victim) = entries.values.keys().next().cloned() {
                entries.values.remove(&victim);
            }
        }
        entries.values.insert(key, (session, Instant::now()));
    }

    /// 同步切换代次并清空条目；在失效之前开始的查询不能恢复任何缓存权限。
    pub(super) fn invalidate(&self) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        let removed = entries.values.len();
        entries.generation = entries.generation.wrapping_add(1);
        entries.values.clear();
        if removed > 0 {
            metrics::counter!("session_cache_invalidations_total").increment(removed as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use uuid::Uuid;

    /// 构造权限快照，过期时间可由用例覆盖。
    fn session() -> Session {
        Session {
            id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            dept_id: None,
            ip: None,
            user_agent: None,
            data_scope: "self".into(),
            username: "test".into(),
            permissions: vec!["system:user:list".into()],
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(1),
            must_change_password: false,
        }
    }

    #[test]
    fn revoked_generation_cannot_be_repopulated() {
        let cache = SessionCache::default();
        let (started, _) = cache.get("token");
        cache.invalidate();
        cache.insert("token".into(), session(), started);
        assert!(cache.get("token").1.is_none());
        let (current, _) = cache.get("token");
        cache.insert("token".into(), session(), current);
        assert!(cache.get("token").1.is_some());
        cache.invalidate();
        assert!(cache.get("token").1.is_none());
    }

    #[test]
    fn ttl_and_session_expiry_are_enforced() {
        let cache = SessionCache::default();
        cache.insert("token".into(), session(), 0);
        cache
            .entries
            .write()
            .expect("lock")
            .values
            .get_mut("token")
            .expect("cached")
            .1 = Instant::now() - SESSION_CACHE_TTL;
        assert!(cache.get("token").1.is_none());
        let mut expired = session();
        expired.expires_at = Utc::now() - Duration::seconds(1);
        cache.insert("expired".into(), expired, 0);
        assert!(cache.get("expired").1.is_none());
    }

    #[test]
    fn cache_does_not_grow_past_capacity() {
        let cache = SessionCache::default();
        let value = session();
        for index in 0..=MAX_ENTRIES {
            cache.insert(index.to_string(), value.clone(), 0);
        }
        assert_eq!(
            cache.entries.read().expect("lock").values.len(),
            MAX_ENTRIES
        );
    }
}
