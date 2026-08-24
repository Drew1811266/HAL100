import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { ApplicationErrorBoundary } from "./ApplicationErrorBoundary";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 5_000,
    },
  },
});

function StartupReady({ onReady }: { onReady: () => void }) {
  useLayoutEffect(() => {
    onReady();
  }, [onReady]);
  return null;
}

export function mountApplication(onReady: () => void = () => undefined) {
  const root = document.getElementById("root");

  if (!root) {
    throw new Error("HAL100 root element was not found");
  }

  const applicationRoot = createRoot(root);
  applicationRoot.render(
    <StrictMode>
      <StartupReady onReady={onReady} />
      <ApplicationErrorBoundary>
        <QueryClientProvider client={queryClient}>
          <BrowserRouter>
            <App />
          </BrowserRouter>
        </QueryClientProvider>
      </ApplicationErrorBoundary>
    </StrictMode>,
  );
  return applicationRoot;
}
