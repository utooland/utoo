import styles from "./index.css";
console.log(styles);
import pkg from "../mako.config.json" with { type: "json"};
console.log(pkg);
import('./a.css');
require('./b.css');
import "./c.css";

// css modules
import d from './d.css';
d;
const e = require('./e.css');
e;
// import('./f.css').then(f => f);
