import { useQuery } from "@tanstack/react-query";
import { useAuthStore } from "../stores/authStore";

export type SystemStats = {
  uptime: number;
  total_connections: number;
  bad_connections: number;
};

export type TrafficPoint = {
  time: string;
  octets_from: number;
  octets_to: number;
};

export type TrafficResponse = {
  hourly: TrafficPoint[];
};

function authHeaders(): HeadersInit {
  const token = useAuthStore.getState().token;
  return token ? { Authorization: `Bearer ${token}` } : {};
}

async function get<T>(path: string): Promise<T> {
  const r = await fetch(path, { headers: authHeaders() });
  if (!r.ok) throw new Error(String(r.status));
  return r.json();
}

export function useSystemStats() {
  const token = useAuthStore((s) => s.token);
  return useQuery({
    queryKey: ["system", "stats", token],
    queryFn: () => get<SystemStats>("/api/system/stats"),
    enabled: !!token,
    refetchInterval: 10_000,
  });
}

export function useTraffic(hours: number = 24) {
  const token = useAuthStore((s) => s.token);
  return useQuery({
    queryKey: ["system", "traffic", hours, token],
    queryFn: () => get<TrafficResponse>(`/api/system/traffic?hours=${hours}`),
    enabled: !!token,
    refetchInterval: 30_000,
  });
}
