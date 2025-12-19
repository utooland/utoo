const fs = require("fs");
const path = require("path");

const total = 20;
const content = `import react from 'react';
import reactDom from 'react-dom';
import * as antd from 'antd';
import * as three from 'three';
import * as lodash from 'lodash';
import * as axios from 'axios';

window.React = react;
window.ReactDom = reactDom;
window.Antd = antd;
window.Three = three;
window.Lodash = lodash;
window.Axios = axios;
`;

const projectOptions = {
  rootPath: "../../",
  projectPath: "./",
  config: {
    entry: [],
    output: {
      path: "./dist",
    },
    stats: true
  },
};

for (let i = 0; i < total; i++) {
  const name = `entry-${i}`;
  const filename = `${name}.js`;

  fs.writeFileSync(path.join(__dirname, "./src", filename), content);

  projectOptions.config.entry.push({
    import: `./src/entry-${i}.js`,
  });

  fs.writeFileSync(
    path.join(__dirname, "utoopack.json"),
    JSON.stringify(projectOptions, null, 2),
  );
}
