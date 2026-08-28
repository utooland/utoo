/**
 * @jsxRuntime classic
 * @jsx h
 * @jsxFrag Fragment
 */
import { Fragment, h } from "./factory";
import { Heading } from "./types";

const heading: Heading = "Custom classic JSX factory";

export default function App() {
  return (
    <>
      <h1>{heading}</h1>
    </>
  );
}
