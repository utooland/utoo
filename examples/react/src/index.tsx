import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { TypeA } from "./typea";

let x: TypeA<string> = { content: "wd" };

x;

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
