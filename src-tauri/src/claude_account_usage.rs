//! Account-wide subscription limits. Credentials are read only inside the VPS
//! helper and sent only to Anthropic's fixed HTTPS usage endpoint. This private
//! endpoint can change; unavailable data is never represented as zero usage.
use crate::engine::wsl_transport::{self, WslTransport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Instant;

const POSITIVE_TTL: Duration = Duration::from_secs(60);
const FAILURE_BACKOFF: Duration = Duration::from_secs(30);
const MAX_CACHED_ACCOUNTS: usize = 64;
const PROBE_PY: &str = include_str!("claude_account_usage.py");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub used_percent: f64,
    pub resets_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccountUsage {
    pub plan: Option<String>,
    pub five_hour: Option<UsageWindow>,
    pub seven_day: Option<UsageWindow>,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct AccountKey {
    host: String,
    user: String,
    port: u16,
    identity: Option<String>,
}

impl From<&WslTransport> for AccountKey {
    fn from(transport: &WslTransport) -> Self {
        Self {
            host: transport.host.clone(),
            user: transport.user.clone(),
            port: transport.port,
            identity: transport.identity_file.clone(),
        }
    }
}

struct CachedResult {
    result: Result<ClaudeAccountUsage, String>,
    completed_at: Instant,
}

#[derive(Default)]
struct AccountUsageCache {
    entries: Mutex<HashMap<AccountKey, Arc<AsyncMutex<Option<CachedResult>>>>>,
}

impl AccountUsageCache {
    fn entry(&self, key: AccountKey) -> Result<Arc<AsyncMutex<Option<CachedResult>>>, String> {
        let mut entries = self.entries.lock().map_err(|_| unavailable())?;
        if let Some(entry) = entries.get(&key) {
            return Ok(Arc::clone(entry));
        }
        if entries.len() >= MAX_CACHED_ACCOUNTS {
            // A map-only Arc has no active request or waiter. Never evict a
            // live entry: that could create a second fetch for the same account.
            let idle = entries
                .iter()
                .find_map(|(key, entry)| (Arc::strong_count(entry) == 1).then(|| key.clone()));
            let Some(idle) = idle else {
                return Err(unavailable());
            };
            entries.remove(&idle);
        }
        let entry = Arc::new(AsyncMutex::new(None));
        entries.insert(key, Arc::clone(&entry));
        Ok(entry)
    }

    async fn get<F, Fut>(
        &self,
        key: AccountKey,
        force: bool,
        fetch: F,
    ) -> Result<ClaudeAccountUsage, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<ClaudeAccountUsage, String>>,
    {
        // A force request arriving during a fetch shares that completed result;
        // a later force request may bypass only the successful-value TTL.
        let requested_at = Instant::now();
        let entry = self.entry(key)?;
        let mut cached = entry.lock().await;
        if let Some(hit) = cached.as_ref() {
            let age = Instant::now().saturating_duration_since(hit.completed_at);
            if hit.completed_at >= requested_at
                || (hit.result.is_err() && age < FAILURE_BACKOFF)
                || (!force && hit.result.is_ok() && age < POSITIVE_TTL)
            {
                return hit.result.clone();
            }
        }
        let result = fetch().await;
        *cached = Some(CachedResult {
            result: result.clone(),
            completed_at: Instant::now(),
        });
        result
    }
}

static CACHE: LazyLock<AccountUsageCache> = LazyLock::new(AccountUsageCache::default);

fn unavailable() -> String {
    "Claude subscription usage is unavailable. Please try again later.".into()
}

fn safe_error(code: &str) -> String {
    match code {
        "auth_required" | "auth_expired" | "forbidden" => {
            "Claude subscription usage requires a valid Claude login on the VPS. Please sign in there again.".into()
        }
        "rate_limited" => {
            "Claude usage checks are temporarily rate limited. Please wait before refreshing.".into()
        }
        _ => unavailable(),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProbeResponse {
    Usage(ClaudeAccountUsage),
    Error { error: String },
}

fn decode_response(raw: &str) -> Result<ClaudeAccountUsage, String> {
    // The helper exports only the normalized summary, never the upstream body.
    if raw.len() > 16 * 1024 {
        return Err(unavailable());
    }
    let response: ProbeResponse = serde_json::from_str(raw).map_err(|_| unavailable())?;
    let mut usage = match response {
        ProbeResponse::Usage(usage) => usage,
        ProbeResponse::Error { error } => return Err(safe_error(&error)),
    };
    for window in [&mut usage.five_hour, &mut usage.seven_day] {
        if window.as_ref().is_some_and(|value| {
            !value.used_percent.is_finite() || !(0.0..=100.0).contains(&value.used_percent)
        }) {
            *window = None;
        }
        if let Some(window) = window {
            if window
                .resets_at
                .is_some_and(|value| value == 0 || value > 9_007_199_254_740_991)
            {
                window.resets_at = None;
            }
        }
    }
    if !usage
        .plan
        .as_deref()
        .is_some_and(|value| matches!(value, "pro" | "max" | "team" | "enterprise"))
    {
        usage.plan = None;
    }
    // Use the Mac's clock for UI freshness, independently of VPS clock skew.
    usage.updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable())?
        .as_millis() as u64;
    Ok(usage)
}

fn probe_script() -> String {
    format!("python3 - <<'CCGUI_ACCOUNT_USAGE'\n{PROBE_PY}\nCCGUI_ACCOUNT_USAGE\n")
}

async fn fetch_remote_usage(transport: &WslTransport) -> Result<ClaudeAccountUsage, String> {
    let raw = wsl_transport::run_readonly_script_output(transport, &probe_script())
        .await
        // SSH/profile diagnostics must not become a credential-bearing UI error.
        .map_err(|_| {
            "Could not read Claude usage over SSH. Please check the VPS connection.".to_string()
        })?;
    decode_response(&raw)
}

#[tauri::command]
pub async fn claude_account_usage(
    state: tauri::State<'_, crate::AppState>,
    workspace_path: String,
    force: bool,
) -> Result<ClaudeAccountUsage, String> {
    let transport = wsl_transport::transport_for_path(&state.db, &workspace_path)?
        .filter(|transport| transport.distro.is_empty())
        .ok_or("Claude subscription usage is available for registered Linux SSH workspaces.")?;
    CACHE
        .get(AccountKey::from(&transport), force, || {
            fetch_remote_usage(&transport)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn key() -> AccountKey {
        AccountKey {
            host: "host".into(),
            user: "dev".into(),
            port: 22,
            identity: None,
        }
    }

    fn usage() -> ClaudeAccountUsage {
        ClaudeAccountUsage {
            plan: Some("pro".into()),
            five_hour: None,
            seven_day: None,
            updated_at: 1,
        }
    }

    #[test]
    fn normalized_payload_preserves_zero_and_unknown_without_inventing_remaining_usage() {
        let parsed = decode_response(r#"{"plan":"pro","fiveHour":{"usedPercent":0,"resetsAt":1792056599627},"sevenDay":null,"updatedAt":1}"#).unwrap();
        assert_eq!(parsed.five_hour.as_ref().unwrap().used_percent, 0.0);
        assert_eq!(parsed.five_hour.unwrap().resets_at, Some(1792056599627));
        assert_eq!(parsed.seven_day, None);
        let parsed = decode_response(r#"{"plan":"unexpected","fiveHour":{"usedPercent":101,"resetsAt":0},"sevenDay":{"usedPercent":41,"resetsAt":null},"updatedAt":1}"#).unwrap();
        assert_eq!(parsed.plan, None);
        assert_eq!(parsed.five_hour, None);
        assert_eq!(parsed.seven_day.unwrap().used_percent, 41.0);
    }

    #[test]
    fn errors_do_not_echo_remote_bodies_or_unknown_error_strings() {
        for raw in [r#"{"error":"auth_expired"}"#, r#"{"error":"forbidden"}"#] {
            assert!(decode_response(raw).unwrap_err().contains("sign in"));
        }
        assert!(decode_response(r#"{"error":"rate_limited"}"#)
            .unwrap_err()
            .contains("wait"));
        let error = decode_response(r#"{"error":"Bearer secret upstream body"}"#).unwrap_err();
        assert!(!error.contains("secret"));
        assert!(!decode_response("Bearer private response")
            .unwrap_err()
            .contains("private"));
    }

    #[tokio::test(start_paused = true)]
    async fn positive_ttl_and_force_refresh_do_not_turn_failures_into_a_click_storm() {
        let cache = AccountUsageCache::default();
        let calls = AtomicUsize::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(usage())
        };
        cache.get(key(), false, fetch).await.unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
        cache.get(key(), false, fetch).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cache.get(key(), true, fetch).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        tokio::time::advance(Duration::from_secs(61)).await;
        cache.get(key(), false, fetch).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        tokio::time::advance(Duration::from_secs(1)).await;
        let fail = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(unavailable())
        };
        cache.get(key(), true, fail).await.unwrap_err();
        tokio::time::advance(Duration::from_secs(1)).await;
        cache.get(key(), true, fail).await.unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        tokio::time::advance(Duration::from_secs(30)).await;
        cache.get(key(), true, fail).await.unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_forced_requests_share_the_same_account_fetch() {
        let cache = Arc::new(AccountUsageCache::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..12 {
            let cache = Arc::clone(&cache);
            let calls = Arc::clone(&calls);
            workers.push(tokio::spawn(async move {
                cache
                    .get(key(), true, || async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        Ok(usage())
                    })
                    .await
            }));
        }
        for worker in workers {
            worker.await.unwrap().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn different_ssh_accounts_never_reuse_each_others_cache() {
        let cache = AccountUsageCache::default();
        let calls = AtomicUsize::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(usage())
        };
        let original = key();
        let mut keys = vec![original.clone()];
        let mut changed = original.clone();
        changed.host = "other-host".into();
        keys.push(changed);
        let mut changed = original.clone();
        changed.user = "other-user".into();
        keys.push(changed);
        let mut changed = original.clone();
        changed.port = 2222;
        keys.push(changed);
        let mut changed = original;
        changed.identity = Some("/other/key".into());
        keys.push(changed);
        for key in keys {
            cache.get(key, false, fetch).await.unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn cache_stays_bounded_without_evicting_in_flight_accounts() {
        let cache = AccountUsageCache::default();
        let mut active = Vec::new();
        for index in 0..MAX_CACHED_ACCOUNTS {
            let mut account = key();
            account.host = format!("host-{index}");
            active.push(cache.entry(account).unwrap());
        }
        let mut first = key();
        first.host = "host-0".into();
        let existing = cache.entry(first.clone()).unwrap();
        assert!(Arc::ptr_eq(&existing, &active[0]));
        assert!(cache.entry(key()).is_err());
        drop(active);
        cache.entry(key()).unwrap();
        assert_eq!(cache.entries.lock().unwrap().len(), MAX_CACHED_ACCOUNTS);
        assert!(Arc::ptr_eq(&existing, &cache.entry(first).unwrap()));
    }
}
