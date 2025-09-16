import React from 'react';
import ReactDOM from 'react-dom/client';
import { Button } from 'antd';

const App = () => {
  return (
    <div>
      <h1>Ant Design Test</h1>
      <Button type="primary">Test Button</Button>
    </div>
  );
};

ReactDOM.createRoot(document.getElementById('root')!).render(<App />);
