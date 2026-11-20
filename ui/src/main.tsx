import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";

import "@fontsource-variable/jetbrains-mono/wght.css";

import { App } from "./App";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 2_000, retry: 1 } },
});

document.addEventListener("contextmenu", (e) => {
  const editable = e.target instanceof Element && e.target.closest("input, textarea, [contenteditable]");
  if (!editable && !window.getSelection()?.toString()) e.preventDefault();
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
