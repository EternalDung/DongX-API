import { createHashRouter, RouterProvider } from "react-router-dom";
import Layout from "./components/layout/Layout";

import { DashboardPage } from "./pages/DashboardPage";
import { UsagePage } from "./pages/UsagePage";
import { ChannelsPage } from "./pages/ChannelsPage";
import { ApiKeysPage } from "./pages/ApiKeysPage";
import { LogsPage } from "./pages/LogsPage";
import { SettingsPage } from "./pages/SettingsPage";
import { ServicesPage } from "./pages/ServicesPage";

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
      { path: "settings", element: <SettingsPage /> },
      { path: "services", element: <ServicesPage /> },
    ],
  },
]);

export default function App() {
  return <RouterProvider router={router} />;
}
