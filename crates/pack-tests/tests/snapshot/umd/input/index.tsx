console.log('hello here');

// @ts-ignore
import React from 'react';
// @ts-ignore
import ReactDOM from 'react-dom';

import { a } from './a.ts';

console.log(a);

function App({content}:{content:string}) {
  // @ts-ignore
  return <div>{content}</div>;
}

// @ts-ignore
const root = ReactDOM.createRoot(document.getElementById('root'));
root.render(<App content={'hello'}/>);