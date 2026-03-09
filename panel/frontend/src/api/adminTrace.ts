import { useQuery } from "@tanstack/react-query";
import { useAuthStore } from "../stores/authStore";

export type TraceSessionSummary = {
  conn_id: number;
  user: string;
  target_dc: number;
  client_addr: string;
  our_addr: string;
  event_count: number;
  last_event_at_ms: number;
  last_event: string;
  state: string;
  closed_at_ms: number | null;
};

export type TraceEvent = {
  seq: number;
  timestamp_ms: number;
  kind: string;
  message: string;
};

export type TraceSessionDump = {
  conn_id: number;
  user: string;
  target_dc: number;
  client_addr: string;
  our_addr: string;
  state: string;
  closed_at_ms: number | null;
  events: TraceEvent[];
};

function authHeaders(): HeadersInit {
  const token = useAuthStore.getState().token;
  return token ? { Authorization: `Bearer ${token}` } : {};
}

async function get<T>(path: string): Promise<T> {
  const response = await fetch(path, { headers: authHeaders() });
  if (!response.ok) {
    const err = await response.json().catch(() => ({}));
    throw new Error((err as { detail?: string }).detail ?? String(response.status));
  }
  return response.json();
}

export function useTraceSessions(params: {
  user?: string;
  dc?: string;
  limit?: number;
}) {
  const token = useAuthStore((s) => s.token);
  const query = new URLSearchParams();
  query.set("limit", String(params.limit ?? 50));
  if (params.user) query.set("user", params.user);
  if (params.dc) query.set("dc", params.dc);
  return useQuery({
    queryKey: ["admin-trace-sessions", params, token],
    queryFn: async () =>
      get<{ sessions: TraceSessionSummary[] }>(`/api/admin/trace/sessions?${query}`).then(
        (payload) => payload.sessions
      ),
    enabled: !!token,
    refetchInterval: 5_000,
  });
}

export function useTraceSession(connId: number | null, limit: number = 200) {
  const token = useAuthStore((s) => s.token);
  return useQuery({
    queryKey: ["admin-trace-session", connId, limit, token],
    queryFn: () => get<TraceSessionDump>(`/api/admin/trace/${connId}?limit=${limit}`),
    enabled: !!token && connId !== null,
    refetchInterval: 5_000,
  });
}
