import React from "react";
import ReactDOM from "react-dom/client";
import "./main.css";

function App() {
  return (
    <main className="page-shell">
      <section className="hero-card">
        <span className="eyebrow">PostCSS Merged Config</span>
        <h1>postcss-nested + px2rem</h1>
        <p className="lede">
          This example loads <code>postcss-nested</code> from
          <code>postcss.config.js</code>, then appends
          <code>postcss-plugin-px2rem</code> from
          <code>styles.postcss.plugins</code>.
        </p>

        <div className="stats-row">
          <div className="stat-box">
            <strong>24px</strong>
            <span>headline size</span>
          </div>
          <div className="stat-box">
            <strong>32px</strong>
            <span>section padding</span>
          </div>
          <div className="stat-box">
            <strong>12px</strong>
            <span>button radius</span>
          </div>
        </div>

        <button className="cta-button" type="button">
          Inspect the built CSS
        </button>
      </section>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
