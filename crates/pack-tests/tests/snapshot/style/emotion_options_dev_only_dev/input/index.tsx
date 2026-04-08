import { css } from "@emotion/react";

const buttonStyle = css`
  color: hotpink;
  font-weight: bold;
`;

function NamedButton() {
  return <button css={buttonStyle}>Click me</button>;
}

export default function App() {
  return (
    <div>
      <NamedButton />
    </div>
  );
}
