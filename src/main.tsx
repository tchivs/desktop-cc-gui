import React from "react";
import ReactDOM from "react-dom/client";
import { preloadClientStores } from "./services/clientStorage";
import { migrateLocalStorageToFileStore } from "./services/migrateLocalStorage";
import { initInputHistoryStore } from "./features/composer/hooks/useInputHistoryStore";
import { ErrorBoundary } from "./components/ErrorBoundary";

function renderBootstrapFallback(error: unknown) {
  const root = document.getElementById("root");
  if (!root) {
    console.error("[bootstrap] Failed before root mount and root element is missing:", error);
    return;
  }

  const errorMessage = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <div
        style={{
          position: "fixed",
          inset: 0,
          background: "#0d0f14",
          color: "#e2e8f0",
          fontFamily: "ui-monospace, monospace",
          fontSize: 13,
          padding: 32,
          overflow: "auto",
        }}
      >
        <h2 style={{ color: "#f87171", margin: "0 0 12px", fontSize: 18 }}>Application Startup Error</h2>
        <p style={{ color: "#94a3b8", margin: "0 0 16px" }}>
          The app failed to initialize. Please reload and try again.
        </p>
        <button
          type="button"
          onClick={() => window.location.reload()}
          style={{
            padding: "8px 16px",
            background: "#1e293b",
            color: "#e2e8f0",
            border: "1px solid #334155",
            borderRadius: 6,
            cursor: "pointer",
            fontSize: 13,
            marginBottom: 16,
          }}
        >
          Reload
        </button>
        <pre
          style={{
            margin: 0,
            padding: 12,
            background: "#1e1e2e",
            borderRadius: 6,
            overflow: "auto",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            fontSize: 12,
            lineHeight: 1.5,
            color: "#f87171",
          }}
        >
          {errorMessage}
        </pre>
      </div>
    </React.StrictMode>,
  );
}

async function bootstrap() {
  await preloadClientStores();
  try {
    migrateLocalStorageToFileStore();
  } catch (error) {
    console.error("[bootstrap] localStorage migration failed, continue startup:", error);
  }
  await initInputHistoryStore();
  // i18n must be imported after preload so language can be read from cache
  await import("./i18n");
  const { default: App } = await import("./App");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </React.StrictMode>,
  );
}

bootstrap().catch((error) => {
  console.error("[bootstrap] Startup failed:", error);
  renderBootstrapFallback(error);
});
