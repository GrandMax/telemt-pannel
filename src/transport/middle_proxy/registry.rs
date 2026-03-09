use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tokio::sync::{mpsc, RwLock};

use crate::trace::{TraceBuffer, TraceEvent, now_ms};

use super::codec::WriterCommand;
use super::MeResponse;

#[derive(Clone)]
pub struct ConnMeta {
    pub user: String,
    pub target_dc: i16,
    pub client_addr: SocketAddr,
    pub our_addr: SocketAddr,
    pub proto_flags: u32,
    pub trace: Option<std::sync::Arc<TraceBuffer>>,
}

#[derive(Clone)]
pub struct BoundConn {
    pub conn_id: u64,
    pub meta: ConnMeta,
}

#[derive(Clone)]
pub struct ConnWriter {
    pub writer_id: u64,
    pub tx: mpsc::Sender<WriterCommand>,
}

struct RegistryInner {
    map: HashMap<u64, mpsc::Sender<MeResponse>>,
    writers: HashMap<u64, mpsc::Sender<WriterCommand>>,
    writer_for_conn: HashMap<u64, u64>,
    conns_for_writer: HashMap<u64, HashSet<u64>>,
    meta: HashMap<u64, ConnMeta>,
    recent: VecDeque<RecentTraceSession>,
}

impl RegistryInner {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            writers: HashMap::new(),
            writer_for_conn: HashMap::new(),
            conns_for_writer: HashMap::new(),
            meta: HashMap::new(),
            recent: VecDeque::new(),
        }
    }
}

#[derive(Clone)]
struct RecentTraceSession {
    conn_id: u64,
    meta: ConnMeta,
    closed_at_ms: u128,
}

#[derive(Clone, Serialize)]
pub struct TraceSessionSummary {
    pub conn_id: u64,
    pub user: String,
    pub target_dc: i16,
    pub client_addr: String,
    pub our_addr: String,
    pub event_count: usize,
    pub last_event_at_ms: u128,
    pub last_event: String,
    pub state: String,
    pub closed_at_ms: Option<u128>,
}

#[derive(Clone, Serialize)]
pub struct TraceSessionDump {
    pub conn_id: u64,
    pub user: String,
    pub target_dc: i16,
    pub client_addr: String,
    pub our_addr: String,
    pub state: String,
    pub closed_at_ms: Option<u128>,
    pub events: Vec<TraceEvent>,
}

const RECENT_TRACE_LIMIT: usize = 128;

pub struct ConnRegistry {
    inner: RwLock<RegistryInner>,
    next_id: AtomicU64,
}

impl ConnRegistry {
    pub fn new() -> Self {
        let start = rand::random::<u64>() | 1;
        Self {
            inner: RwLock::new(RegistryInner::new()),
            next_id: AtomicU64::new(start),
        }
    }

    pub async fn register(&self) -> (u64, mpsc::Receiver<MeResponse>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(1024);
        self.inner.write().await.map.insert(id, tx);
        (id, rx)
    }

    /// Unregister connection, returning associated writer_id if any.
    pub async fn unregister(&self, id: u64) -> Option<u64> {
        let mut inner = self.inner.write().await;
        inner.map.remove(&id);
        if let Some(meta) = inner.meta.remove(&id)
            && meta.trace.is_some()
        {
            inner.recent.push_back(RecentTraceSession {
                conn_id: id,
                meta,
                closed_at_ms: now_ms(),
            });
            while inner.recent.len() > RECENT_TRACE_LIMIT {
                inner.recent.pop_front();
            }
        }
        if let Some(writer_id) = inner.writer_for_conn.remove(&id) {
            if let Some(set) = inner.conns_for_writer.get_mut(&writer_id) {
                set.remove(&id);
            }
            return Some(writer_id);
        }
        None
    }

    pub async fn route(&self, id: u64, resp: MeResponse) -> bool {
        let inner = self.inner.read().await;
        if let Some(tx) = inner.map.get(&id) {
            tx.try_send(resp).is_ok()
        } else {
            false
        }
    }

    pub async fn bind_writer(
        &self,
        conn_id: u64,
        writer_id: u64,
        tx: mpsc::Sender<WriterCommand>,
        meta: ConnMeta,
    ) {
        let mut inner = self.inner.write().await;
        inner.meta.entry(conn_id).or_insert(meta);
        inner.writer_for_conn.insert(conn_id, writer_id);
        inner.writers.entry(writer_id).or_insert_with(|| tx.clone());
        inner
            .conns_for_writer
            .entry(writer_id)
            .or_insert_with(HashSet::new)
            .insert(conn_id);
    }

    pub async fn get_writer(&self, conn_id: u64) -> Option<ConnWriter> {
        let inner = self.inner.read().await;
        let writer_id = inner.writer_for_conn.get(&conn_id).cloned()?;
        let writer = inner.writers.get(&writer_id).cloned()?;
        Some(ConnWriter { writer_id, tx: writer })
    }

    pub async fn writer_lost(&self, writer_id: u64) -> Vec<BoundConn> {
        let mut inner = self.inner.write().await;
        inner.writers.remove(&writer_id);
        let conns = inner
            .conns_for_writer
            .remove(&writer_id)
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();

        let mut out = Vec::new();
        for conn_id in conns {
            inner.writer_for_conn.remove(&conn_id);
            if let Some(m) = inner.meta.get(&conn_id) {
                out.push(BoundConn {
                    conn_id,
                    meta: m.clone(),
                });
            }
        }
        out
    }

    pub async fn get_meta(&self, conn_id: u64) -> Option<ConnMeta> {
        let inner = self.inner.read().await;
        inner.meta.get(&conn_id).cloned()
    }

    pub async fn push_trace(&self, conn_id: u64, event: TraceEvent) {
        let trace = {
            let inner = self.inner.read().await;
            inner.meta.get(&conn_id).and_then(|meta| meta.trace.clone())
        };
        if let Some(trace) = trace {
            trace.push(event);
        }
    }

    pub async fn push_trace_for_writer(&self, writer_id: u64, event: TraceEvent) {
        let traces = {
            let inner = self.inner.read().await;
            inner
                .conns_for_writer
                .get(&writer_id)
                .into_iter()
                .flat_map(|conn_ids| conn_ids.iter())
                .filter_map(|conn_id| inner.meta.get(conn_id).and_then(|meta| meta.trace.clone()))
                .collect::<Vec<_>>()
        };
        for trace in traces {
            trace.push(event.clone());
        }
    }

    pub async fn is_writer_empty(&self, writer_id: u64) -> bool {
        let inner = self.inner.read().await;
        inner
            .conns_for_writer
            .get(&writer_id)
            .map(|s| s.is_empty())
            .unwrap_or(true)
    }

    pub async fn list_trace_sessions(&self, limit: Option<usize>) -> Vec<TraceSessionSummary> {
        let inner = self.inner.read().await;
        let mut sessions = Vec::new();

        for (conn_id, meta) in &inner.meta {
            if let Some(trace) = &meta.trace {
                sessions.push(build_summary(*conn_id, meta, trace, "active", None));
            }
        }
        for recent in &inner.recent {
            if let Some(trace) = &recent.meta.trace {
                sessions.push(build_summary(
                    recent.conn_id,
                    &recent.meta,
                    trace,
                    "recent",
                    Some(recent.closed_at_ms),
                ));
            }
        }

        sessions.sort_by(|a, b| b.last_event_at_ms.cmp(&a.last_event_at_ms));
        if let Some(limit) = limit {
            sessions.truncate(limit);
        }
        sessions
    }

    pub async fn get_trace_session(&self, conn_id: u64, limit: Option<usize>) -> Option<TraceSessionDump> {
        let inner = self.inner.read().await;
        if let Some(meta) = inner.meta.get(&conn_id)
            && let Some(trace) = &meta.trace
        {
            return Some(build_dump(conn_id, meta, trace, "active", None, limit));
        }
        let recent = inner.recent.iter().find(|item| item.conn_id == conn_id)?;
        let trace = recent.meta.trace.as_ref()?;
        Some(build_dump(
            conn_id,
            &recent.meta,
            trace,
            "recent",
            Some(recent.closed_at_ms),
            limit,
        ))
    }
}

fn build_summary(
    conn_id: u64,
    meta: &ConnMeta,
    trace: &TraceBuffer,
    state: &str,
    closed_at_ms: Option<u128>,
) -> TraceSessionSummary {
    let latest = trace
        .latest()
        .unwrap_or_else(|| TraceEvent::new("empty", "no events"));
    TraceSessionSummary {
        conn_id,
        user: meta.user.clone(),
        target_dc: meta.target_dc,
        client_addr: meta.client_addr.to_string(),
        our_addr: meta.our_addr.to_string(),
        event_count: trace.len(),
        last_event_at_ms: latest.timestamp_ms,
        last_event: latest.short(),
        state: state.to_string(),
        closed_at_ms,
    }
}

fn build_dump(
    conn_id: u64,
    meta: &ConnMeta,
    trace: &TraceBuffer,
    state: &str,
    closed_at_ms: Option<u128>,
    limit: Option<usize>,
) -> TraceSessionDump {
    TraceSessionDump {
        conn_id,
        user: meta.user.clone(),
        target_dc: meta.target_dc,
        client_addr: meta.client_addr.to_string(),
        our_addr: meta.our_addr.to_string(),
        state: state.to_string(),
        closed_at_ms,
        events: trace.snapshot(limit),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;

    use tokio::sync::mpsc;

    use crate::trace::{TraceBuffer, TraceEvent};

    use super::{ConnMeta, ConnRegistry};

    #[tokio::test]
    async fn bind_writer_preserves_trace_buffer_in_meta() {
        let registry = ConnRegistry::new();
        let (conn_id, _rx) = registry.register().await;
        let (tx, _writer_rx) = mpsc::channel(1);
        let trace = Arc::new(TraceBuffer::new(4));

        registry
            .bind_writer(
                conn_id,
                7,
                tx,
                ConnMeta {
                    user: "alice".to_string(),
                    target_dc: 2,
                    client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1000),
                    our_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2000),
                    proto_flags: 0x10,
                    trace: Some(trace.clone()),
                },
            )
            .await;

        let meta = registry.get_meta(conn_id).await.expect("meta");
        let trace = meta.trace.expect("trace buffer");
        trace.push(TraceEvent::new("bind", "writer attached"));

        let snapshot = trace.snapshot(None);
        assert_eq!(meta.user, "alice");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].kind, "bind");
    }

    #[tokio::test]
    async fn unregistered_trace_session_is_kept_in_recent_list() {
        let registry = ConnRegistry::new();
        let (conn_id, _rx) = registry.register().await;
        let (tx, _writer_rx) = mpsc::channel(1);
        let trace = Arc::new(TraceBuffer::new(4));
        trace.push(TraceEvent::new("start", "opened"));

        registry
            .bind_writer(
                conn_id,
                8,
                tx,
                ConnMeta {
                    user: "bob".to_string(),
                    target_dc: 4,
                    client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000),
                    our_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4000),
                    proto_flags: 0x20,
                    trace: Some(trace.clone()),
                },
            )
            .await;

        let _ = registry.unregister(conn_id).await;

        let sessions = registry.list_trace_sessions(None).await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].conn_id, conn_id);
        assert_eq!(sessions[0].state, "recent");

        let dump = registry.get_trace_session(conn_id, Some(10)).await.expect("dump");
        assert_eq!(dump.user, "bob");
        assert_eq!(dump.events.len(), 1);
        assert_eq!(dump.events[0].kind, "start");
    }
}
