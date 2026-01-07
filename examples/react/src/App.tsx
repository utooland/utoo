import React from "react";
import Person from "../static/person.svg";
import { foo } from "./foo.ts";
import styles from "./index.module.less";

export function App() {
  return (
    <>
      <h1>React Version is: {React.version}</h1>
      <Person />
    </>
  );
}
