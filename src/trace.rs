use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceEvent {
    pub seq: u64,
    pub timestamp_ms: u128,
    pub kind: String,
    pub message: String,
}

impl TraceEvent {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            seq: 0,
            timestamp_ms: now_ms(),
            kind: kind.into(),
            message: message.into(),
        }
    }

    pub fn short(&self) -> String {
        format!("#{} {} {}", self.seq, self.kind, self.message)
    }
}

#[derive(Debug)]
pub struct TraceBuffer {
    capacity: usize,
    next_seq: AtomicU64,
    events: Mutex<VecDeque<TraceEvent>>,
}

impl TraceBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_seq: AtomicU64::new(1),
            events: Mutex::new(VecDeque::with_capacity(capacity.max(1))),
        }
    }

    pub fn push(&self, mut event: TraceEvent) {
        event.seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut events = self.events.lock().unwrap();
        if events.len() >= self.capacity {
            events.pop_front();
        }
        events.push_back(event);
    }

    pub fn snapshot(&self, limit: Option<usize>) -> Vec<TraceEvent> {
        let events = self.events.lock().unwrap();
        let start = limit
            .map(|limit| events.len().saturating_sub(limit))
            .unwrap_or(0);
        events.iter().skip(start).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    pub fn latest(&self) -> Option<TraceEvent> {
        self.events.lock().unwrap().back().cloned()
    }
}

pub fn push_if_enabled(trace: Option<&TraceBuffer>, event: TraceEvent) {
    if let Some(trace) = trace {
        trace.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_buffer_keeps_only_latest_events() {
        let trace = TraceBuffer::new(2);

        trace.push(TraceEvent::new("connect", "conn opened"));
        trace.push(TraceEvent::new("frame", "first payload"));
        trace.push(TraceEvent::new("ack", "quick ack"));

        let snapshot = trace.snapshot(None);
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].kind, "frame");
        assert_eq!(snapshot[1].kind, "ack");
    }

    #[test]
    fn trace_event_short_formats_kind_and_message() {
        let event = TraceEvent::new("rpc_proxy_ans", "flags=1 len=128");
        let short = event.short();

        assert!(short.contains("rpc_proxy_ans"));
        assert!(short.contains("flags=1 len=128"));
    }

    #[test]
    fn disabled_trace_helper_is_noop() {
        push_if_enabled(None, TraceEvent::new("noop", "disabled"));
    }
}
