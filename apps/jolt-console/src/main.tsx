import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ConsoleApp } from "./app/App";
import "./styles.css";

const root = document.querySelector<HTMLDivElement>("#app");
if (!root) {
  throw new Error("missing #app root");
}

createRoot(root).render(
  <StrictMode>
    <ConsoleApp />
  </StrictMode>
);
