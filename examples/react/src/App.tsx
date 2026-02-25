import React, { useState, Suspense, lazy } from "react";
import Person from "./person.svg";
import "./App.less";

// Create a lazy loaded component
const LazyComponent = lazy(() => import('./LazyComponent'));

export function App() {
  const [count, setCount] = useState(0);
  const [showLazy, setShowLazy] = useState(false);

  return (
    <div className="appContainer">
      <header className="header">
        <div className="logoContainer">
          <Person className="logo" width={40} height={40} />
          <h1 className="title">Welcome to React + Utoo</h1>
        </div>
        <p className="subtitle">React Engine: v{React.version}</p>
      </header>

      <main className="mainContent">
        <section className="card">
          <div className="cardHeader">
            <h2>Interactive Demo</h2>
          </div>
          <div className="cardBody">
            <p>This is a richer React demo featuring modern CSS styles, state management, and elegant layout.</p>
            <div className="counterSection">
              <p className="counterText">Current count: <strong>{count}</strong></p>
              <div className="buttonGroup">
                <button 
                  className="button" 
                  onClick={() => setCount(c => c + 1)}
                >
                  Increase
                </button>
                <button 
                  className="button buttonOutline" 
                  onClick={() => setCount(0)}
                >
                  Reset
                </button>
              </div>
            </div>

            <div className="lazySection" style={{ marginTop: '2rem', borderTop: '1px dashed #e2e8f0', paddingTop: '1.5rem' }}>
              <button 
                className="button buttonOutline" 
                onClick={() => setShowLazy(prev => !prev)}
                style={{ width: '100%', marginBottom: '1rem' }}
              >
                {showLazy ? 'Hide Lazy Component' : 'Load Lazy Component'}
              </button>
              
              {showLazy && (
                <Suspense fallback={<div style={{ textAlign: 'center', padding: '1rem', color: '#64748b' }}>Loading custom component...</div>}>
                  <LazyComponent />
                </Suspense>
              )}
            </div>
          </div>
        </section>

        <section className="card">
          <div className="cardHeader">
            <h2>Features Showcase</h2>
          </div>
          <div className="cardBody">
            <ul className="featureList">
              <li>✨ Fast Refresh & Hot Module Replacement</li>
              <li>🎨 Modern Less CSS (No Modules)</li>
              <li>📦 React.lazy Component Splitting</li>
              <li>🚀 Optimized Production Builds</li>
              <li>📦 Seamless TypeScript Integration</li>
            </ul>
          </div>
        </section>
      </main>
      
      <footer className="footer">
        <p>Built with ❤️ using React</p>
      </footer>
    </div>
  );
}
