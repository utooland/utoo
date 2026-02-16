'use client';

import { useState, useEffect } from 'react';

export function Counter() {
  const [count, setCount] = useState(0);

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
      <button
        onClick={() => setCount(c => c - 1)}
        style={{
          width: 40, height: 40, borderRadius: 8,
          border: '1px solid #333', background: '#1a1a1a', color: '#fff',
          fontSize: 20, cursor: 'pointer',
        }}
      >-</button>
      <span style={{ fontSize: 32, fontWeight: 700, fontVariantNumeric: 'tabular-nums', minWidth: 48, textAlign: 'center' }}>
        {count}
      </span>
      <button
        onClick={() => setCount(c => c + 1)}
        style={{
          width: 40, height: 40, borderRadius: 8,
          border: '1px solid #333', background: '#1a1a1a', color: '#fff',
          fontSize: 20, cursor: 'pointer',
        }}
      >+</button>
    </div>
  );
}

export function Clock() {
  const [time, setTime] = useState('');

  useEffect(() => {
    const tick = () => setTime(new Date().toLocaleTimeString());
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, []);

  return (
    <span style={{ fontVariantNumeric: 'tabular-nums', fontWeight: 500 }}>
      {time || '--:--:--'}
    </span>
  );
}
