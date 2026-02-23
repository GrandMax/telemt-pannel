import { create } from "zustand";
import { persist } from "zustand/middleware";

export const useAuthStore = create<{
  token: string | null;
  setToken: (t: string | null) => void;
}>()(
  persist(
    (set) => ({
      token: null,
      setToken: (token) => set({ token }),
    }),
    { name: "mtpannel-auth" }
  )
);
