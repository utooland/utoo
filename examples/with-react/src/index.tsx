import { ClickToComponent } from "click-to-react-component";
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <>
    <ClickToComponent editor="vscode" />
    <App />
  </>,
);
