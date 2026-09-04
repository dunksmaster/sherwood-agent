import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError } from "../api.ts";

export interface Poll<T> {
  data: T | null;
  error: ApiError | null;
  loading: boolean;
  refresh: () => void;
}

/**
 * Call `fn` now and every `intervalMs`. `onUnauthorized` fires once when a 401
 * is seen so the caller can drop the token.
 */
export function usePoll<T>(
  fn: () => Promise<T>,
  intervalMs: number,
  onUnauthorized?: () => void,
): Poll<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [loading, setLoading] = useState(true);
  const fnRef = useRef(fn);
  fnRef.current = fn;
  const unauthRef = useRef(onUnauthorized);
  unauthRef.current = onUnauthorized;

  const run = useCallback(async () => {
    try {
      const next = await fnRef.current();
      setData(next);
      setError(null);
    } catch (e) {
      const err = e instanceof ApiError ? e : new ApiError(0, "unknown", String(e));
      setError(err);
      if (err.status === 401) unauthRef.current?.();
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void run();
    const id = setInterval(() => void run(), intervalMs);
    return () => clearInterval(id);
  }, [run, intervalMs]);

  return { data, error, loading, refresh: () => void run() };
}
