import { createHashRouter, RouterProvider } from "react-router-dom";
import Layout from "./components/layout/Layout";

import { DashboardPage } from "./pages/DashboardPage";
import { UsagePage } from "./pages/UsagePage";
import { ChannelsPage } from "./pages/ChannelsPage";
import { ApiKeysPage } from "./pages/ApiKeysPage";
import { LogsPage } from "./pages/LogsPage";
import { AuditPage } from "./pages/AuditPage";
import { SettingsPage } from "./pages/SettingsPage";

const router = createHashRouter([
  {
    path: "/",
    element: <Layout />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: "usage", element: <UsagePage /> },
      { path: "channels", element: <ChannelsPage /> },
      { path: "api-keys", element: <ApiKeysPage /> },
      { path: "logs", element: <LogsPage /> },
      { path: "audit", element: <AuditPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);

export default function App() {
  return <RouterProvider router={router} />;
}
