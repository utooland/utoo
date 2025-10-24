console.log('hello here');

// @ts-ignore
import React from 'react';
// @ts-ignore
import ReactDOM from 'react-dom';

/** @jsxImportSource @emotion/react */
import { jsx } from '@emotion/react'

import { styled } from '@emotion/styled';

console.log(styled);

import { a } from './a.ts';

console.log(a);

function App({content}:{content:string}) {
  // @ts-ignore
  return <div>{content}</div>;
}

// @ts-ignore
const root = ReactDOM.createRoot(document.getElementById('root'));
root.render(<App content={'hello'}/>);