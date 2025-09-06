import React from "react";
import Person from "../static/person.svg";
import { foo } from "./foo.ts";
import styles from "./index.module.less";
import dataText from "./test.txt";

export function App() {
  return (
    <>
      <h1 className={styles.pre}>
        App2 {foo} - HMR Test by {dataText}
      </h1>
      <h1>React Version is: {React.version}</h1>
      <Person />
    </>
  );
}
