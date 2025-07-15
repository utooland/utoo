/** @jsxImportSource @emotion/react */
import { jsx } from '@emotion/react'

type IProps = {
    content: string;
}

function App(props: IProps) {
    // @ts-ignore make lsp happy.
    return <div>{props.content}</div>;
}

export default App;