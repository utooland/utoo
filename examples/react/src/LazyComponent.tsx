import React from 'react';
import './LazyComponent.less';

export default function LazyComponent() {
  return (
    <div className="lazy-container">
      <h3 className="lazy-title">Lazy Loaded Component</h3>
      <p className="lazy-content">
        This component is loaded lazily! This helps reduce the initial bundle size
        and improves the performance of your application.
      </p>
      <div className="lazy-decoration">
        <div className="lazy-dot"></div>
        <div className="lazy-dot"></div>
        <div className="lazy-dot"></div>
      </div>
    </div>
  );
}
