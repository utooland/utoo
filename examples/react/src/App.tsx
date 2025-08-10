import React from "react";
import Person from "../static/person.svg";
import { foo } from "./foo.ts";
import dataText from "./test.txt";

export function App() {
  return (
    <>
      <h1>
        App {foo} - HMR Test by {dataText}
      </h1>
      <Person />
    </>
  );
}
