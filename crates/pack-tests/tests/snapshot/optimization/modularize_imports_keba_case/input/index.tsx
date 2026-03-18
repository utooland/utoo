import { Button, DatePicker } from "antd";
import { UploadProps } from "antd";
import type { UploadFile } from "antd";
import React from 'react';

const props: UploadProps = {
    name: "test"
}

const file: UploadFile = {
    name: "file"
}

console.log('props', props);
console.log('file', file);

console.log(Button);
console.log(DatePicker);

const App = () => {
    return (
        <div>
            <Button>Click</Button>
        </div>
    )
}

export default App;