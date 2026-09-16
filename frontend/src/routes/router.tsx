import { Navigate, Route, Routes } from "react-router-dom";
import { useToken } from "../hooks/useToken.ts";
import { NavShell } from "../components/NavShell.tsx";
import { LandingPage } from "../pages/LandingPage.tsx";
import { LoginPage } from "../pages/LoginPage.tsx";
import { DashboardPage } from "../pages/DashboardPage.tsx";
import { SettingsPage } from "../pages/SettingsPage.tsx";
import { RequireToken } from "./RequireToken.tsx";

/**
 * The single `useToken()` call for the whole app. It's a plain hook, not
 * context — calling it again in a page component would create a second,
 * disconnected copy of the token instead of sharing this one, so `token`/
 * `setToken`/`clear` are threaded down as props from here.
 */
export function AppRoutes() {
  const { token, setToken, clear } = useToken();

  return (
    <Routes>
      <Route path="/" element={<LandingPage />} />
      <Route
        path="/login"
        element={token ? <Navigate to="/app" replace /> : <LoginPage setToken={setToken} />}
      />
      <Route
        path="/app"
        element={
          <RequireToken token={token}>
            <NavShell onLogout={clear}>
              <DashboardPage token={token as string} onLogout={clear} />
            </NavShell>
          </RequireToken>
        }
      />
      <Route
        path="/settings"
        element={
          <RequireToken token={token}>
            <NavShell onLogout={clear}>
              <SettingsPage />
            </NavShell>
          </RequireToken>
        }
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
