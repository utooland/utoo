import React, { useState } from "react";
import { label } from "./value";
import "./style.css";

export default function App() {
  const [count, setCount] = useState(0);
  return (
    <main>
      <span id="label">{label}</span>
      <button id="increment" onClick={() => setCount(count + 1)}>
        Count: {count}
      </button>
    </main>
  );
}
