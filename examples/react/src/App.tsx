import React from 'react';
import { foo } from './foo.ts';
import dataText from './test.txt';

export function App() {
  return <h1>App {foo} - HMR Test by {dataText}</h1>;
}
