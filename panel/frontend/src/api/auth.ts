import { useMutation, useQuery } from "@tanstack/react-query";
import { useAuthStore } from "../stores/authStore";

type TokenResponse = { access_token: string; token_type: string };

export function useLogin() {
  const setToken = useAuthStore((s) => s.setToken);
  return useMutation({
    mutationFn: async (body: { username: string; password: string }) => {
      const form = new URLSearchParams({
        username: body.username,
        password: body.password,
      });
      const r = await fetch("/api/admin/token", {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: form.toString(),
      });
      if (!r.ok) {
        const err = await r.json().catch(() => ({}));
        throw new Error((err as { detail?: string }).detail ?? "Login failed");
      }
      return r.json() as Promise<TokenResponse>;
    },
    onSuccess: (data) => setToken(data.access_token),
  });
}

export function useMe() {
  const token = useAuthStore((s) => s.token);
  return useQuery({
    queryKey: ["me", token],
    queryFn: async () => {
      const r = await fetch("/api/admin/me", {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!r.ok) throw new Error("Unauthorized");
      return r.json();
    },
    enabled: !!token,
  });
}
