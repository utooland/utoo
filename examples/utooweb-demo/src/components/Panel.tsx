import React from 'react';

interface PanelProps {
  title: React.ReactNode;
  children: React.ReactNode;
  actions?: React.ReactNode;
  style?: React.CSSProperties;
  contentStyle?: React.CSSProperties;
}

export const Panel: React.FC<PanelProps> = ({ title, children, actions, style, contentStyle }) => {
  return (
    <div
      style={{
        backgroundColor: '#ffffff',
        display: 'flex',
        flexDirection: 'column',
        boxSizing: 'border-box',
        ...style,
      }}
    >
      <div
        style={{
          padding: '0.5rem 1rem',
          borderBottom: '1px solid #e5e7eb',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          flexShrink: 0,
          height: '2.5rem',
          boxSizing: 'border-box',
        }}
      >
        <h3 style={{ fontSize: '0.875rem', fontWeight: '600', margin: 0 }}>
          {title}
        </h3>
        {actions && <div>{actions}</div>}
      </div>
      <div
        style={{
          flex: 1,
          overflow: 'auto',
          position: 'relative',
          ...contentStyle,
        }}
      >
        {children}
      </div>
    </div>
  );
};
