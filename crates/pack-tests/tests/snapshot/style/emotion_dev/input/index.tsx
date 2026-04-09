import { css } from "@emotion/react";

const cardStyle = css`
  padding: 8px;
  border: 1px solid #ddd;
`;

function Card() {
  return <section css={cardStyle}>Card</section>;
}

export default function App() {
  return <Card />;
}
