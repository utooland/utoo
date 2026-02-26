import React from "react";

const LazyComponent: React.FC = () => {
  return (
    <div
      className="card"
      style={{ borderColor: "#28a745", borderStyle: "dashed" }}
    >
      <h2 style={{ color: "#28a745" }}>Lazy Loaded Component</h2>
      <p>
        I was loaded asynchronously using <code>React.lazy</code> and{" "}
        <code>Suspense</code>!
      </p>
      <p>
        This demonstrates that <strong>dynamic imports</strong> work correctly
        through the compatibility layer.
      </p>
    </div>
  );
};

export default LazyComponent;
