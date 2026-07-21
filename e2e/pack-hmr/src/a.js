import { sharedValue } from "./shared.js";
import "./shared.css";

document.querySelector("#a").textContent = `a:${sharedValue}`;

if (import.meta.turbopackHot) {
  import.meta.turbopackHot.accept();
}
