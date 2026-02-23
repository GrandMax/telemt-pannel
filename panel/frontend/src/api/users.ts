import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useAuthStore } from "../stores/authStore";

export type User = {
  id: number;
  username: string;
  secret: string;
  status: string;
  data_limit: number | null;
  data_used: number;
  max_connections: number | null;
  max_unique_ips: number | null;
  expire_at: string | null;
  note: string | null;
  created_at: string;
  proxy_links?: { tg_link: string; https_link: string };
};

export type UserCreateInput = {
  username: string;
  data_limit?: number | null;
  max_connections?: number | null;
  max_unique_ips?: number | null;
  expire_at?: string | null;
  note?: string | null;
};

export type UserUpdateInput = {
  data_limit?: number | null;
  max_connections?: number | null;
  max_unique_ips?: number | null;
  expire_at?: string | null;
  status?: string | null;
  note?: string | null;
};

type ListResult = { users: User[]; total: number };

function authHeaders(): HeadersInit {
  const token = useAuthStore.getState().token;
  return token ? { Authorization: `Bearer ${token}` } : {};
}

async function get(path: string) {
  const r = await fetch(path, { headers: authHeaders() });
  if (!r.ok) {
    const err = await r.json().catch(() => ({}));
    throw new Error((err as { detail?: string }).detail ?? String(r.status));
  }
  return r.json();
}

async function post(path: string, body?: object) {
  const r = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!r.ok) {
    const err = await r.json().catch(() => ({}));
    throw new Error((err as { detail?: string }).detail ?? String(r.status));
  }
  return r.status === 204 ? undefined : r.json();
}

async function put(path: string, body: object) {
  const r = await fetch(path, {
    method: "PUT",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const err = await r.json().catch(() => ({}));
    throw new Error((err as { detail?: string }).detail ?? String(r.status));
  }
  return r.json();
}

async function del(path: string) {
  const r = await fetch(path, { method: "DELETE", headers: authHeaders() });
  if (!r.ok) {
    const err = await r.json().catch(() => ({}));
    throw new Error((err as { detail?: string }).detail ?? String(r.status));
  }
}

export function useUsersList(params: {
  offset: number;
  limit: number;
  search?: string;
  status?: string;
}) {
  const token = useAuthStore((s) => s.token);
  const searchParams = new URLSearchParams();
  searchParams.set("offset", String(params.offset));
  searchParams.set("limit", String(params.limit));
  if (params.search) searchParams.set("search", params.search);
  if (params.status) searchParams.set("status", params.status);
  return useQuery({
    queryKey: ["users", params, token],
    queryFn: () => get(`/api/users?${searchParams}`) as Promise<ListResult>,
    enabled: !!token,
  });
}

export function useUser(username: string | null) {
  const token = useAuthStore((s) => s.token);
  return useQuery({
    queryKey: ["user", username, token],
    queryFn: () => get(`/api/users/${encodeURIComponent(username!)}`) as Promise<User>,
    enabled: !!token && !!username,
  });
}

export function useCreateUser() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: UserCreateInput) =>
      post("/api/users", body) as Promise<User>,
    onSuccess: () => qc.invalidateQueries({ queryKey: ["users"] }),
  });
}

export function useUpdateUser() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ username, ...body }: { username: string } & UserUpdateInput) =>
      put(`/api/users/${encodeURIComponent(username)}`, body) as Promise<User>,
    onSuccess: (_, v) => {
      qc.invalidateQueries({ queryKey: ["users"] });
      qc.invalidateQueries({ queryKey: ["user", v.username] });
    },
  });
}

export function useDeleteUser() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (username: string) =>
      del(`/api/users/${encodeURIComponent(username)}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["users"] }),
  });
}

export function useRegenerateSecret() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (username: string) =>
      post(`/api/users/${encodeURIComponent(username)}/regenerate-secret`) as Promise<User>,
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ["users"] });
      qc.invalidateQueries({ queryKey: ["user", data.username] });
    },
  });
}

export function useUserLinks(username: string | null) {
  const token = useAuthStore((s) => s.token);
  return useQuery({
    queryKey: ["user-links", username, token],
    queryFn: () =>
      get(`/api/users/${encodeURIComponent(username!)}/links`) as Promise<{
        tg_link: string;
        https_link: string;
      }>,
    enabled: !!token && !!username,
  });
}
