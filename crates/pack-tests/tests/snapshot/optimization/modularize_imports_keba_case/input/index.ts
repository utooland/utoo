import { Button, DatePicker } from "antd";
import { UploadProps } from "antd";
import type { UploadFile } from "antd";

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