import type { ReactNode } from "react";
import { Link, NavLink } from "react-router-dom";

/**
 * A secondary nav strip for authenticated routes (`/app`, `/settings`) — not
 * a `.topbar`. `DashboardPage` already renders its own `.topbar` via
 * `<StatusBar>` (brand + mode/kill-switch/uptime); reusing `.topbar` here
 * too would stack two sticky headers on `/app`. `/settings` has no
 * `StatusBar`, so this strip carries a small brand link for identity there.
 */
export function NavShell({
  onLogout,
  children,
}: {
  onLogout: () => void;
  children: ReactNode;
}) {
  const navClass = ({ isActive }: { isActive: boolean }) =>
    "nav-link" + (isActive ? " nav-link-active" : "");

  return (
    <>
      <div className="subnav">
        <Link to="/" className="subnav-brand">
          sherwood
        </Link>
        <nav className="row" aria-label="Primary">
          <NavLink to="/app" className={navClass} end>
            Dashboard
          </NavLink>
          <NavLink to="/settings" className={navClass}>
            Settings
          </NavLink>
          <button onClick={onLogout}>Disconnect</button>
        </nav>
      </div>
      {children}
    </>
  );
}
