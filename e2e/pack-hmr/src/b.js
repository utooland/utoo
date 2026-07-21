import { sharedValue } from "./shared.js";
import "./shared.css";

document.querySelector("#b").textContent = `b:${sharedValue}`;

if (import.meta.turbopackHot) {
  import.meta.turbopackHot.accept();
}
