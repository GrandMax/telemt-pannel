import { useMutation } from "@tanstack/react-query";
import { useAuthStore } from "../stores/authStore";

export type ExportSettings = {
  proxy_host: string;
  proxy_port: number;
  tls_domain: string;
  telemt_metrics_url: string | null;
  telemt_ignore_time_skew: boolean;
};

export type ExportUser = {
  username: string;
  secret: string;
  enabled: boolean;
  ip_limit: number | null;
  comment: string | null;
  status?: string | null;
  data_limit?: number | null;
  max_connections?: number | null;
  expire_at?: string | null;
};

export type ExportSnapshot = {
  version: number;
  exported_at: string;
  settings: ExportSettings;
  users: ExportUser[];
};

export type ImportSkippedItem = {
  username: string | null;
  reason: string;
};

export type ImportReport = {
  added: number;
  updated: number;
  skipped: ImportSkippedItem[];
};

function authHeaders(): HeadersInit {
  const token = useAuthStore.getState().token;
  return token ? { Authorization: `Bearer ${token}` } : {};
}

async function parseError(response: Response): Promise<never> {
  const err = await response.json().catch(() => ({}));
  throw new Error((err as { detail?: string }).detail ?? String(response.status));
}

async function getExportSnapshot(): Promise<ExportSnapshot> {
  const response = await fetch("/api/admin/export", {
    headers: authHeaders(),
  });
  if (!response.ok) {
    return parseError(response);
  }
  return response.json();
}

async function postImportSnapshot(args: {
  mode: "merge" | "replace";
  snapshot: ExportSnapshot;
}): Promise<ImportReport> {
  const response = await fetch(`/api/admin/import?mode=${args.mode}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...authHeaders(),
    },
    body: JSON.stringify(args.snapshot),
  });
  if (!response.ok) {
    return parseError(response);
  }
  return response.json();
}

export function useExportSnapshot() {
  return useMutation({
    mutationFn: getExportSnapshot,
  });
}

export function useImportSnapshot() {
  return useMutation({
    mutationFn: postImportSnapshot,
  });
}
