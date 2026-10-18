//! Native Linux short RPCs share a per-server connection budget. Interactive
//! Claude/PTY sessions retain their dedicated SSH lifecycle and cancellation.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_HOSTS: usize = 64;
const MAX_CONNECTIONS: usize = 3;
const QUEUE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Eq, Hash, PartialEq)]
struct HostKey {
    host: String,
    port: u16,
}

#[derive(Debug)]
struct HostPool {
    connections: Arc<Semaphore>,
    bulk: Arc<Semaphore>,
}

impl Default for HostPool {
    fn default() -> Self {
        Self {
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            bulk: Arc::new(Semaphore::new(1)),
        }
    }
}

/// Keeps the map entry active until the RPC and its permits have been dropped.
#[derive(Debug)]
pub(crate) struct RpcPermit {
    _connections: OwnedSemaphorePermit,
    _bulk: Option<OwnedSemaphorePermit>,
    _pool: Arc<HostPool>,
}

#[derive(Default)]
struct RpcPool {
    hosts: Mutex<HashMap<HostKey, Arc<HostPool>>>,
}

impl RpcPool {
    fn host(&self, host: &str, port: u16) -> Result<Arc<HostPool>, String> {
        let key = HostKey {
            host: host.to_ascii_lowercase(),
            port,
        };
        let mut hosts = self
            .hosts
            .lock()
            .map_err(|_| "Remote SSH queue is unavailable".to_string())?;
        if let Some(pool) = hosts.get(&key) {
            return Ok(Arc::clone(pool));
        }
        if hosts.len() >= MAX_HOSTS {
            // An acquired/queued request holds an Arc, including requests
            // running on the dedicated synchronous file/Git runtimes.
            let idle = hosts
                .iter()
                .find(|(_, pool)| Arc::strong_count(pool) == 1)
                .map(|(key, _)| HostKey {
                    host: key.host.clone(),
                    port: key.port,
                });
            match idle {
                Some(key) => {
                    hosts.remove(&key);
                }
                None => return Err("Remote SSH queue is full; try again shortly".into()),
            }
        }
        let pool = Arc::new(HostPool::default());
        hosts.insert(key, Arc::clone(&pool));
        Ok(pool)
    }

    async fn acquire(&self, host: &str, port: u16, bulk: bool) -> Result<RpcPermit, String> {
        let pool = self.host(host, port)?;
        let permits = tokio::time::timeout(QUEUE_TIMEOUT, async {
            // Waiting bulk calls must not consume the general permits. With
            // one active bulk request, two slots remain available to short RPCs.
            let bulk_permit = if bulk {
                Some(Arc::clone(&pool.bulk).acquire_owned().await)
            } else {
                None
            };
            let bulk_permit = bulk_permit.transpose().map_err(|_| unavailable())?;
            let connections = Arc::clone(&pool.connections)
                .acquire_owned()
                .await
                .map_err(|_| unavailable())?;
            Ok::<_, String>((connections, bulk_permit))
        })
        .await
        .map_err(|_| "Remote SSH queue timed out (60s); try again shortly".to_string())??;
        Ok(RpcPermit {
            _connections: permits.0,
            _bulk: permits.1,
            _pool: pool,
        })
    }
}

fn unavailable() -> String {
    "Remote SSH queue is unavailable".into()
}

static POOL: LazyLock<RpcPool> = LazyLock::new(RpcPool::default);

pub(crate) async fn acquire(host: &str, port: u16, bulk: bool) -> Result<RpcPermit, String> {
    POOL.acquire(host, port, bulk).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_server_shares_three_slots_while_other_ports_and_hosts_are_independent() {
        let pool = Arc::new(RpcPool::default());
        let first = pool.acquire("VPS.example", 22, false).await.unwrap();
        let second = pool.acquire("vps.example", 22, false).await.unwrap();
        let third = pool.acquire("VPS.example", 22, false).await.unwrap();
        let queued = {
            let pool = Arc::clone(&pool);
            tokio::spawn(async move { pool.acquire("vps.example", 22, false).await })
        };
        tokio::task::yield_now().await;
        assert!(!queued.is_finished(), "fourth same-server RPC must wait");
        let other_port = pool.acquire("vps.example", 2222, false).await.unwrap();
        let other_host = pool.acquire("other.example", 22, false).await.unwrap();
        drop(first);
        let fourth = queued.await.unwrap().unwrap();
        assert_eq!(fourth._pool.connections.available_permits(), 0);
        drop((second, third, fourth, other_port, other_host));
        assert_eq!(
            pool.host("vps.example", 22)
                .unwrap()
                .connections
                .available_permits(),
            3
        );
    }

    #[tokio::test]
    async fn waiting_bulk_requests_leave_two_short_rpc_slots_available() {
        let pool = Arc::new(RpcPool::default());
        let active_bulk = pool.acquire("host", 22, true).await.unwrap();
        let waiting_bulk = {
            let pool = Arc::clone(&pool);
            tokio::spawn(async move { pool.acquire("host", 22, true).await })
        };
        tokio::task::yield_now().await;
        assert!(!waiting_bulk.is_finished());
        let short_a = pool.acquire("host", 22, false).await.unwrap();
        let short_b = pool.acquire("host", 22, false).await.unwrap();
        assert_eq!(active_bulk._pool.connections.available_permits(), 0);
        drop(active_bulk);
        let next_bulk = waiting_bulk.await.unwrap().unwrap();
        assert_eq!(next_bulk._pool.bulk.available_permits(), 0);
        drop((short_a, short_b, next_bulk));
        assert_eq!(
            pool.host("host", 22)
                .unwrap()
                .connections
                .available_permits(),
            3
        );
    }

    #[tokio::test(start_paused = true)]
    async fn queue_timeout_releases_bulk_reservation_before_returning_an_error() {
        let pool = Arc::new(RpcPool::default());
        let a = pool.acquire("host", 22, false).await.unwrap();
        let b = pool.acquire("host", 22, false).await.unwrap();
        let c = pool.acquire("host", 22, false).await.unwrap();
        let waiting = {
            let pool = Arc::clone(&pool);
            tokio::spawn(async move { pool.acquire("host", 22, true).await })
        };
        tokio::task::yield_now().await;
        let host = pool.host("host", 22).unwrap();
        assert_eq!(host.bulk.available_permits(), 0);
        tokio::time::advance(QUEUE_TIMEOUT).await;
        let error = waiting.await.unwrap().err().expect("queue timeout");
        assert!(error.contains("queue timed out (60s)"));
        assert_eq!(host.bulk.available_permits(), 1);
        drop((a, b, c));
        assert!(pool.acquire("host", 22, true).await.is_ok());
    }

    #[tokio::test]
    async fn cancellation_and_rpc_errors_release_all_permits() {
        let pool = Arc::new(RpcPool::default());
        let active_bulk = pool.acquire("host", 22, true).await.unwrap();
        let waiting = {
            let pool = Arc::clone(&pool);
            tokio::spawn(async move { pool.acquire("host", 22, true).await })
        };
        tokio::task::yield_now().await;
        waiting.abort();
        assert!(waiting.await.unwrap_err().is_cancelled());
        let active = {
            let pool = Arc::clone(&pool);
            tokio::spawn(async move {
                let _permit = pool.acquire("host", 22, false).await.unwrap();
                std::future::pending::<()>().await;
            })
        };
        tokio::task::yield_now().await;
        active.abort();
        assert!(active.await.unwrap_err().is_cancelled());
        drop(active_bulk);
        let failed_rpc = async {
            let _permit = pool.acquire("host", 22, true).await?;
            Err::<(), String>("simulated SSH error".into())
        };
        assert!(failed_rpc.await.is_err());
        let host = pool.host("host", 22).unwrap();
        assert_eq!(host.connections.available_permits(), 3);
        assert_eq!(host.bulk.available_permits(), 1);
    }

    #[tokio::test]
    async fn host_map_is_bounded_and_never_evicts_active_or_queued_hosts() {
        let pool = RpcPool::default();
        let mut active = Vec::new();
        for i in 0..MAX_HOSTS {
            active.push(pool.acquire(&format!("host-{i}"), 22, false).await.unwrap());
        }
        let existing = pool.acquire("host-0", 22, false).await.unwrap();
        assert!(pool.acquire("new-host", 22, false).await.is_err());
        assert_eq!(pool.hosts.lock().unwrap().len(), MAX_HOSTS);
        drop(active.pop());
        let newcomer = pool.acquire("new-host", 22, false).await.unwrap();
        assert_eq!(pool.hosts.lock().unwrap().len(), MAX_HOSTS);
        assert!(pool.hosts.lock().unwrap().contains_key(&HostKey {
            host: "host-0".into(),
            port: 22,
        }));
        drop((existing, newcomer, active));
    }

    #[test]
    fn blocking_worker_runtimes_share_the_same_per_server_budget() {
        let pool = Arc::new(RpcPool::default());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let first = runtime.block_on(pool.acquire("host", 22, false)).unwrap();
        let second = runtime.block_on(pool.acquire("host", 22, false)).unwrap();
        let third = runtime.block_on(pool.acquire("host", 22, false)).unwrap();
        let (queued_tx, queued_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let pending = pool.acquire("host", 22, false);
                tokio::pin!(pending);
                tokio::select! {
                    result = &mut pending => panic!("budget unexpectedly free: {}", result.is_ok()),
                    _ = tokio::task::yield_now() => queued_tx.send(()).unwrap(),
                }
                pending.await.unwrap()
            })
        });
        queued_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(first);
        let fourth = worker.join().unwrap();
        assert_eq!(fourth._pool.connections.available_permits(), 0);
        drop((second, third, fourth));
    }
}
