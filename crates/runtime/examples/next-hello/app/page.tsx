import { Counter, Clock } from './counter';

const features: string[] = [
  'TypeScript first-class support',
  'React Server Components (RSC)',
  'Turbopack hot reload',
  'Server-side rendering',
  'Client-side hydration',
];

export default function Page() {
  const buildTime = new Date().toLocaleTimeString();

  return (
    <div style={{ minHeight: '100vh', background: 'linear-gradient(135deg, #0a0a0a 0%, #1a1a2e 100%)', color: '#e0e0e0' }}>
      <div style={{ maxWidth: 720, margin: '0 auto', padding: '60px 24px' }}>

        <div style={{ marginBottom: 48 }}>
          <h1 style={{ fontSize: 48, fontWeight: 800, margin: 0, color: '#fff' }}>
            utoo-runtime
          </h1>
          <p style={{ fontSize: 20, color: '#888', marginTop: 8 }}>
            Next.js {require('next/package.json').version} &middot; React {require('react/package.json').version} &middot; Turbopack
          </p>
        </div>

        <div style={{
          display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16, marginBottom: 48,
        }}>
          <div style={{ background: '#ffffff0a', borderRadius: 12, padding: 24, border: '1px solid #ffffff15' }}>
            <div style={{ fontSize: 13, color: '#888', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
              SSR Build Time
            </div>
            <div style={{ fontSize: 20, color: '#fff' }}>{buildTime}</div>
          </div>
          <div style={{ background: '#ffffff0a', borderRadius: 12, padding: 24, border: '1px solid #ffffff15' }}>
            <div style={{ fontSize: 13, color: '#888', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
              Client Clock
            </div>
            <div style={{ fontSize: 20, color: '#fff' }}><Clock /></div>
          </div>
        </div>

        <div style={{ background: '#ffffff0a', borderRadius: 12, padding: 32, border: '1px solid #ffffff15', marginBottom: 48 }}>
          <h2 style={{ margin: '0 0 20px', fontSize: 22, color: '#fff' }}>Interactive Counter</h2>
          <Counter />
          <p style={{ color: '#888', fontSize: 14, marginTop: 12, marginBottom: 0 }}>
            Client component with useState -- hydrated from server-rendered HTML.
          </p>
        </div>

        <div style={{ marginBottom: 48 }}>
          <h2 style={{ fontSize: 22, color: '#fff', marginBottom: 16 }}>What is working</h2>
          <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
            {features.map((f, i) => (
              <li key={i} style={{
                padding: '10px 0', borderBottom: '1px solid #ffffff10',
                display: 'flex', alignItems: 'center', gap: 10,
              }}>
                <span style={{ color: '#4ade80', fontSize: 18 }}>&#10003;</span>
                <span>{f}</span>
              </li>
            ))}
          </ul>
        </div>

        <p style={{ color: '#555', fontSize: 13 }}>
          Powered by deno_core v8 + SWC + Turbopack. No polyfills, no hacks.
        </p>
      </div>
    </div>
  );
}
