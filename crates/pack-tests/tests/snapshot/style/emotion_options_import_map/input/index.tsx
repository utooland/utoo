import { css } from "@emotion/react";

const textStyle = css`
  color: rebeccapurple;
`;

export default function App() {
  return <p css={textStyle}>Hello</p>;
}
