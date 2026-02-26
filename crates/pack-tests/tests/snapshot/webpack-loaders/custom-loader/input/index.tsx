// @ts-ignore
import dataText from './data.txt';
// @ts-ignore
import reversedDataText from './test.mdx?reverse';

export default function App() {
    return (
        <div>
            <p id="data-text">{dataText}</p>
            <p id="reversed">{reversedDataText}</p>
        </div>
    );
}