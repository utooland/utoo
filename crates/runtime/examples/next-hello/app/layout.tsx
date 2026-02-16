import type { ReactNode } from 'react';

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <head>
        <title>utoo-runtime | Next.js Demo</title>
        <meta name="description" content="Next.js running on utoo-runtime" />
      </head>
      <body style={{ margin: 0, fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif' }}>
        {children}
      </body>
    </html>
  );
}
