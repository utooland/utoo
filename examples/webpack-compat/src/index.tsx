import React from "react";
import ReactDOM from "react-dom";
import App from "./App";

console.log("Webpack Compat Demo (TS) Initializing...");

const root = document.getElementById("root");
if (root) {
  ReactDOM.render(<App />, root);
}
