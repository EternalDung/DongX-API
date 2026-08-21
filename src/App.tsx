import { createHashRouter, RouterProvider } from "react-router-dom";
import Layout from "./components/layout/Layout";

// Lazy load pages (code splitting)
const DashboardPage = () => import("./pages/DashboardPage").then((m) => ({ default: m.DashboardPage }));
const ChannelsPage = () => import("./pages/ChannelsPage").then((m) => ({ default: m.ChannelsPage }));
const ApiKeysPage = () => import("./pages/ApiKeysPage").then((m) => ({ default: m.ApiKeysPage }));
const LogsPage = () => import("./pages/LogsPage").then((m) => ({ default: m.LogsPage }));
const AuditPage = () => import("./pages/AuditPage").then((m) => ({ default: m.AuditPage }));
const SettingsPage = () => import("./pages/SettingsPage").then((m) => ({ default: m.SettingsPage }));

const router = createHashRouter([
  {
    path: "/",
    element: <Layout />,
    children: [
      { index: true, lazy: DashboardPage },
      { path: "channels", lazy: ChannelsPage },
      { path: "api-keys", lazy: ApiKeysPage },
      { path: "logs", lazy: LogsPage },
      { path: "audit", lazy: AuditPage },
      { path: "settings", lazy: SettingsPage },
    ],
  },
]);

export default function App() {
  return <RouterProvider router={router} />;
}
