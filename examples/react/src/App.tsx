import React from 'react';
import { foo } from './foo.ts';
import dataText from './test.txt';
import Person from '../static/person.svg';
import styles from './index.module.less';

export function App() {
  return <>
    <h1 className={styles.pre}>App2 {foo} - HMR Test by {dataText}</h1>
    <Person />
  </>;
}
