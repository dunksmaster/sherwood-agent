import type { ReactElement } from "react";
import { Navigate, useLocation } from "react-router-dom";

/** Redirects to `/login` (remembering where from) when no token is held. */
export function RequireToken({
  token,
  children,
}: {
  token: string | null;
  children: ReactElement;
}) {
  const location = useLocation();
  if (!token) {
    return <Navigate to="/login" replace state={{ from: location.pathname }} />;
  }
  return children;
}
