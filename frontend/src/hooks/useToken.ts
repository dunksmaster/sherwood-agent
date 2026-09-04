import { useCallback, useState } from "react";

const KEY = "sherwood.token";

/**
 * The bearer token, held in `sessionStorage` so it clears when the tab closes
 * and never touches disk. `clear` is called on a 401.
 */
export function useToken(): {
  token: string | null;
  setToken: (t: string) => void;
  clear: () => void;
} {
  const [token, setTok] = useState<string | null>(() => {
    try {
      return sessionStorage.getItem(KEY);
    } catch {
      return null;
    }
  });

  const setToken = useCallback((t: string) => {
    try {
      sessionStorage.setItem(KEY, t);
    } catch {
      /* private mode — keep it in memory only */
    }
    setTok(t);
  }, []);

  const clear = useCallback(() => {
    try {
      sessionStorage.removeItem(KEY);
    } catch {
      /* ignore */
    }
    setTok(null);
  }, []);

  return { token, setToken, clear };
}
