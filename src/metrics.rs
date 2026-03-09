use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use ipnetwork::IpNetwork;
use serde_json::json;
use tokio::net::TcpListener;
use tracing::{info, warn, debug};

use crate::ip_tracker::UserIpTracker;
use crate::stats::Stats;
use crate::transport::middle_proxy::ConnRegistry;

pub async fn serve(
    port: u16,
    stats: Arc<Stats>,
    ip_tracker: Arc<UserIpTracker>,
    whitelist: Vec<IpNetwork>,
    trace_registry: Option<Arc<ConnRegistry>>,
) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            warn!(error = %e, "Failed to bind metrics on {}", addr);
            return;
        }
    };
    info!("Metrics endpoint: http://{}/metrics", addr);

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "Metrics accept error");
                continue;
            }
        };

        if !whitelist.is_empty() && !whitelist.iter().any(|net| net.contains(peer.ip())) {
            debug!(peer = %peer, "Metrics request denied by whitelist");
            continue;
        }

        let stats = stats.clone();
        let ip_tracker = ip_tracker.clone();
        let trace_registry = trace_registry.clone();
        tokio::spawn(async move {
            let svc = service_fn(move |req| {
                let stats = stats.clone();
                let ip_tracker = ip_tracker.clone();
                let trace_registry = trace_registry.clone();
                let peer = peer;
                async move {
                    let ip_stats = ip_tracker.get_stats().await;
                    handle(req, &stats, &ip_stats, trace_registry, peer).await
                }
            });
            if let Err(e) = http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                .await
            {
                debug!(error = %e, "Metrics connection error");
            }
        });
    }
}

async fn handle<B>(
    req: Request<B>,
    stats: &Stats,
    ip_stats: &[(String, usize, usize)],
    trace_registry: Option<Arc<ConnRegistry>>,
    peer: SocketAddr,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();
    if path == "/metrics" {
        let body = render_metrics(stats, ip_stats);
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
            .body(Full::new(Bytes::from(body)))
            .unwrap();
        return Ok(resp);
    }

    if path == "/trace/sessions" {
        if !is_trace_peer_allowed(peer.ip()) {
            return Ok(json_response(StatusCode::FORBIDDEN, json!({"detail": "Trace endpoint is restricted"})));
        }
        let Some(registry) = trace_registry else {
            return Ok(json_response(StatusCode::NOT_FOUND, json!({"detail": "Trace is disabled"})));
        };
        let limit = parse_limit(req.uri().query());
        let sessions = registry.list_trace_sessions(limit).await;
        return Ok(json_response(StatusCode::OK, json!({ "sessions": sessions })));
    }

    if let Some(conn_id_str) = path.strip_prefix("/trace/")
        && let Ok(conn_id) = conn_id_str.parse::<u64>()
    {
        if !is_trace_peer_allowed(peer.ip()) {
            return Ok(json_response(StatusCode::FORBIDDEN, json!({"detail": "Trace endpoint is restricted"})));
        }
        let Some(registry) = trace_registry else {
            return Ok(json_response(StatusCode::NOT_FOUND, json!({"detail": "Trace is disabled"})));
        };
        let limit = parse_limit(req.uri().query());
        if let Some(session) = registry.get_trace_session(conn_id, limit).await {
            return Ok(json_response(StatusCode::OK, serde_json::to_value(session).unwrap()));
        }
        return Ok(json_response(StatusCode::NOT_FOUND, json!({"detail": "Trace session not found"})));
    }

    let resp = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Full::new(Bytes::from("Not Found\n")))
        .unwrap();
    Ok(resp)
}

fn is_trace_peer_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local(),
    }
}

fn parse_limit(query: Option<&str>) -> Option<usize> {
    let query = query?;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key == "limit"
            && let Ok(limit) = value.parse::<usize>()
        {
            return Some(limit);
        }
    }
    None
}

fn json_response(status: StatusCode, value: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(serde_json::to_vec(&value).unwrap())))
        .unwrap()
}

fn render_metrics(stats: &Stats, ip_stats: &[(String, usize, usize)]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(4096);

    let _ = writeln!(out, "# HELP telemt_uptime_seconds Proxy uptime");
    let _ = writeln!(out, "# TYPE telemt_uptime_seconds gauge");
    let _ = writeln!(out, "telemt_uptime_seconds {:.1}", stats.uptime_secs());

    let _ = writeln!(out, "# HELP telemt_connections_total Total accepted connections");
    let _ = writeln!(out, "# TYPE telemt_connections_total counter");
    let _ = writeln!(out, "telemt_connections_total {}", stats.get_connects_all());

    let _ = writeln!(out, "# HELP telemt_connections_bad_total Bad/rejected connections");
    let _ = writeln!(out, "# TYPE telemt_connections_bad_total counter");
    let _ = writeln!(out, "telemt_connections_bad_total {}", stats.get_connects_bad());

    let _ = writeln!(out, "# HELP telemt_handshake_timeouts_total Handshake timeouts");
    let _ = writeln!(out, "# TYPE telemt_handshake_timeouts_total counter");
    let _ = writeln!(out, "telemt_handshake_timeouts_total {}", stats.get_handshake_timeouts());

    let _ = writeln!(out, "# HELP telemt_me_keepalive_sent_total ME keepalive frames sent");
    let _ = writeln!(out, "# TYPE telemt_me_keepalive_sent_total counter");
    let _ = writeln!(out, "telemt_me_keepalive_sent_total {}", stats.get_me_keepalive_sent());

    let _ = writeln!(out, "# HELP telemt_me_keepalive_failed_total ME keepalive send failures");
    let _ = writeln!(out, "# TYPE telemt_me_keepalive_failed_total counter");
    let _ = writeln!(out, "telemt_me_keepalive_failed_total {}", stats.get_me_keepalive_failed());

    let _ = writeln!(out, "# HELP telemt_me_reconnect_attempts_total ME reconnect attempts");
    let _ = writeln!(out, "# TYPE telemt_me_reconnect_attempts_total counter");
    let _ = writeln!(out, "telemt_me_reconnect_attempts_total {}", stats.get_me_reconnect_attempts());

    let _ = writeln!(out, "# HELP telemt_me_reconnect_success_total ME reconnect successes");
    let _ = writeln!(out, "# TYPE telemt_me_reconnect_success_total counter");
    let _ = writeln!(out, "telemt_me_reconnect_success_total {}", stats.get_me_reconnect_success());

    let _ = writeln!(out, "# HELP telemt_user_connections_total Per-user total connections");
    let _ = writeln!(out, "# TYPE telemt_user_connections_total counter");
    let _ = writeln!(out, "# HELP telemt_user_connections_current Per-user active connections");
    let _ = writeln!(out, "# TYPE telemt_user_connections_current gauge");
    let _ = writeln!(out, "# HELP telemt_user_octets_from_client Per-user bytes received");
    let _ = writeln!(out, "# TYPE telemt_user_octets_from_client counter");
    let _ = writeln!(out, "# HELP telemt_user_octets_to_client Per-user bytes sent");
    let _ = writeln!(out, "# TYPE telemt_user_octets_to_client counter");
    let _ = writeln!(out, "# HELP telemt_user_msgs_from_client Per-user messages received");
    let _ = writeln!(out, "# TYPE telemt_user_msgs_from_client counter");
    let _ = writeln!(out, "# HELP telemt_user_msgs_to_client Per-user messages sent");
    let _ = writeln!(out, "# TYPE telemt_user_msgs_to_client counter");

    for entry in stats.iter_user_stats() {
        let user = entry.key();
        let s = entry.value();
        let _ = writeln!(out, "telemt_user_connections_total{{user=\"{}\"}} {}", user, s.connects.load(std::sync::atomic::Ordering::Relaxed));
        let _ = writeln!(out, "telemt_user_connections_current{{user=\"{}\"}} {}", user, s.curr_connects.load(std::sync::atomic::Ordering::Relaxed));
        let _ = writeln!(out, "telemt_user_octets_from_client{{user=\"{}\"}} {}", user, s.octets_from_client.load(std::sync::atomic::Ordering::Relaxed));
        let _ = writeln!(out, "telemt_user_octets_to_client{{user=\"{}\"}} {}", user, s.octets_to_client.load(std::sync::atomic::Ordering::Relaxed));
        let _ = writeln!(out, "telemt_user_msgs_from_client{{user=\"{}\"}} {}", user, s.msgs_from_client.load(std::sync::atomic::Ordering::Relaxed));
        let _ = writeln!(out, "telemt_user_msgs_to_client{{user=\"{}\"}} {}", user, s.msgs_to_client.load(std::sync::atomic::Ordering::Relaxed));
    }

    let _ = writeln!(out, "# HELP telemt_user_unique_ips_active Per-user count of currently connected unique IPs");
    let _ = writeln!(out, "# TYPE telemt_user_unique_ips_active gauge");
    for (username, active_count, _limit) in ip_stats {
        let _ = writeln!(out, "telemt_user_unique_ips_active{{user=\"{}\"}} {}", username, active_count);
    }

    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::net::Ipv4Addr;

    use http_body_util::BodyExt;
    use tokio::sync::mpsc;

    use super::*;
    use crate::trace::{TraceBuffer, TraceEvent};
    use crate::transport::middle_proxy::{ConnMeta, ConnRegistry};

    #[test]
    fn test_render_metrics_format() {
        let stats = Arc::new(Stats::new());
        stats.increment_connects_all();
        stats.increment_connects_all();
        stats.increment_connects_bad();
        stats.increment_handshake_timeouts();
        stats.increment_user_connects("alice");
        stats.increment_user_curr_connects("alice");
        stats.add_user_octets_from("alice", 1024);
        stats.add_user_octets_to("alice", 2048);
        stats.increment_user_msgs_from("alice");
        stats.increment_user_msgs_to("alice");
        stats.increment_user_msgs_to("alice");

        let output = render_metrics(&stats, &[]);

        assert!(output.contains("telemt_connections_total 2"));
        assert!(output.contains("telemt_connections_bad_total 1"));
        assert!(output.contains("telemt_handshake_timeouts_total 1"));
        assert!(output.contains("telemt_user_connections_total{user=\"alice\"} 1"));
        assert!(output.contains("telemt_user_connections_current{user=\"alice\"} 1"));
        assert!(output.contains("telemt_user_octets_from_client{user=\"alice\"} 1024"));
        assert!(output.contains("telemt_user_octets_to_client{user=\"alice\"} 2048"));
        assert!(output.contains("telemt_user_msgs_from_client{user=\"alice\"} 1"));
        assert!(output.contains("telemt_user_msgs_to_client{user=\"alice\"} 2"));
    }

    #[test]
    fn test_render_empty_stats() {
        let stats = Stats::new();
        let output = render_metrics(&stats, &[]);
        assert!(output.contains("telemt_connections_total 0"));
        assert!(output.contains("telemt_connections_bad_total 0"));
        assert!(output.contains("telemt_handshake_timeouts_total 0"));
        assert!(!output.contains("user="));
    }

    #[test]
    fn test_render_has_type_annotations() {
        let stats = Stats::new();
        let output = render_metrics(&stats, &[]);
        assert!(output.contains("# TYPE telemt_uptime_seconds gauge"));
        assert!(output.contains("# TYPE telemt_connections_total counter"));
        assert!(output.contains("# TYPE telemt_connections_bad_total counter"));
        assert!(output.contains("# TYPE telemt_handshake_timeouts_total counter"));
    }

    #[tokio::test]
    async fn test_endpoint_integration() {
        let stats = Arc::new(Stats::new());
        stats.increment_connects_all();
        stats.increment_connects_all();
        stats.increment_connects_all();

        let req = Request::builder()
            .uri("/metrics")
            .body(())
            .unwrap();
        let resp = handle(
            req,
            &stats,
            &[],
            None,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345),
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(body.as_ref()).unwrap().contains("telemt_connections_total 3"));

        let req404 = Request::builder()
            .uri("/other")
            .body(())
            .unwrap();
        let resp404 = handle(
            req404,
            &stats,
            &[],
            None,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345),
        )
        .await
        .unwrap();
        assert_eq!(resp404.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_trace_sessions_endpoint_returns_recent_sessions() {
        let stats = Arc::new(Stats::new());
        let registry = Arc::new(ConnRegistry::new());
        let (conn_id, _rx) = registry.register().await;
        let (tx, _writer_rx) = mpsc::channel(1);
        let trace = Arc::new(TraceBuffer::new(4));
        trace.push(TraceEvent::new("start", "opened"));
        registry
            .bind_writer(
                conn_id,
                5,
                tx,
                ConnMeta {
                    user: "alice".to_string(),
                    target_dc: 2,
                    client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1000),
                    our_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2000),
                    proto_flags: 0x10,
                    trace: Some(trace),
                },
            )
            .await;
        let _ = registry.unregister(conn_id).await;

        let req = Request::builder()
            .uri("/trace/sessions")
            .body(())
            .unwrap();
        let resp = handle(
            req,
            &stats,
            &[],
            Some(registry),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345),
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(body.as_ref()).unwrap();
        assert_eq!(json["sessions"][0]["conn_id"].as_u64(), Some(conn_id));
        assert_eq!(json["sessions"][0]["state"].as_str(), Some("recent"));
    }

    #[tokio::test]
    async fn test_trace_session_endpoint_honors_limit_and_404() {
        let stats = Arc::new(Stats::new());
        let registry = Arc::new(ConnRegistry::new());
        let (conn_id, _rx) = registry.register().await;
        let (tx, _writer_rx) = mpsc::channel(1);
        let trace = Arc::new(TraceBuffer::new(8));
        trace.push(TraceEvent::new("start", "opened"));
        trace.push(TraceEvent::new("frame", "payload-1"));
        trace.push(TraceEvent::new("ack", "quickack"));
        registry
            .bind_writer(
                conn_id,
                9,
                tx,
                ConnMeta {
                    user: "bob".to_string(),
                    target_dc: 3,
                    client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000),
                    our_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4000),
                    proto_flags: 0x20,
                    trace: Some(trace),
                },
            )
            .await;

        let req = Request::builder()
            .uri(format!("/trace/{conn_id}?limit=2"))
            .body(())
            .unwrap();
        let resp = handle(
            req,
            &stats,
            &[],
            Some(registry.clone()),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345),
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(body.as_ref()).unwrap();
        assert_eq!(json["events"].as_array().map(|v| v.len()), Some(2));
        assert_eq!(json["events"][0]["kind"].as_str(), Some("frame"));
        assert_eq!(json["events"][1]["kind"].as_str(), Some("ack"));

        let req404 = Request::builder()
            .uri("/trace/999999")
            .body(())
            .unwrap();
        let resp404 = handle(
            req404,
            &stats,
            &[],
            Some(registry),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345),
        )
        .await
        .unwrap();
        assert_eq!(resp404.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_trace_endpoints_reject_public_peers() {
        let stats = Arc::new(Stats::new());
        let registry = Arc::new(ConnRegistry::new());
        let req = Request::builder()
            .uri("/trace/sessions")
            .body(())
            .unwrap();

        let resp = handle(
            req,
            &stats,
            &[],
            Some(registry),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 23456),
        )
        .await
        .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
