import React from "react";
import ReactDOM from "react-dom/client";
import { PrimeReactProvider } from "@primereact/core/config";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <PrimeReactProvider unstyled>
      <App />
    </PrimeReactProvider>
  </React.StrictMode>,
);
