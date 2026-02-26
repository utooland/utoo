import React, { lazy, Suspense, useEffect, useState } from "react";
import "./App.css";
import { formatVersion, something } from "@/utils/helper";
import Button from "./components/Button";
import inlineHtml from "./inline.html";
import logo from "./logo.svg";

const LazyComponent = lazy(() => import("./components/LazyComponent"));

const App: React.FC = () => {
  const [count, setCount] = useState(0);
  const [currentTime, setCurrentTime] = useState(
    new Date().toLocaleTimeString(),
  );

  useEffect(() => {
    const timer = setInterval(() => {
      setCurrentTime(new Date().toLocaleTimeString());
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  return (
    <div className="app">
      <header className="header">
        <img src={logo} className="logo" alt="Utoo Logo" />
        <h1>Utoo Webpack Compatibility Demo (TS)</h1>
        <p>
          Demonstrating seamless transformation of Complex Webpack
          configurations.
        </p>
        <div className="version-tag">
          {formatVersion(typeof VERSION !== "undefined" ? VERSION : "unknown")}
        </div>
      </header>

      <main>
        <section className="card">
          <h2>Interactive State</h2>
          <p>
            Current Count: <strong>{count}</strong>
          </p>
          <div style={{ display: "flex", gap: "10px" }}>
            <Button onClick={() => setCount((prev) => prev + 1)}>
              Increment
            </Button>
            <Button variant="secondary" onClick={() => setCount(0)}>
              Reset
            </Button>
          </div>
        </section>

        <Suspense
          fallback={<div className="card">Loading lazy component...</div>}
        >
          <LazyComponent />
        </Suspense>

        <section className="card">
          <h2>Loader Compatibility</h2>
          <div dangerouslySetInnerHTML={{ __html: inlineHtml }} />
        </section>

        <section className="card">
          <h2>Features Demonstrated</h2>
          <ul style={{ paddingLeft: "20px", lineHeight: "1.8" }}>
            <li>
              <strong>TypeScript:</strong> Full type safety for components and
              helpers
            </li>
            <li>
              <strong>Lazy Loading:</strong> Component loaded via dynamic{" "}
              <code>import()</code>
            </li>
            <li>
              <strong>Aliases:</strong> <code>@/utils/helper</code>:{" "}
              <span style={{ color: "#0076ff" }}>{something}</span>
            </li>
            <li>
              <strong>DefinePlugin:</strong> VERSION, process.env.NODE_ENV
            </li>
          </ul>
        </section>
      </main>

      <footer className="code-snippet">
        <h4>System Info</h4>
        <div className="stats-grid">
          <div className="stat-item">
            <strong>Environment:</strong> {process.env.NODE_ENV}
          </div>
          <div className="stat-item">
            <strong>Time:</strong> {currentTime}
          </div>
        </div>
      </footer>
    </div>
  );
};

export default App;
