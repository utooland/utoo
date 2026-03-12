/** @jsxImportSource @emotion/react */
import { css } from "@emotion/react";

const buttonStyle = css`
  color: hotpink;
  font-weight: bold;
`;

const containerStyle = css`
  padding: 16px;
  background: #f0f0f0;
`;

function Button({ children }: { children: React.ReactNode }) {
  return <button css={buttonStyle}>{children}</button>;
}

function App() {
  return (
    <div css={containerStyle}>
      <Button>Click me</Button>
    </div>
  );
}

export default App;
