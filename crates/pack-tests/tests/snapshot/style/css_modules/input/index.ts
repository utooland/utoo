// auto css modules

import styles from './auto_css_modules/style1.css';
import lessStyles from './auto_css_modules/style.less';
import sassStyles from './auto_css_modules/style.sass';
import scssStyles from './auto_css_modules/style.scss';

console.log('CSS styles:', styles);
console.log('LESS styles:', lessStyles);
console.log('SASS styles:', sassStyles);
console.log('SCSS styles:', scssStyles);

import existingModules from './auto_css_modules/style2.css?modules';
console.log('Existing modules:', existingModules); 


// normal css modules
import styles2 from './normal_css_modules/style.module.css';
import lessStyle2 from './normal_css_modules/style.module.less';
import sassStyle2 from './normal_css_modules/style.module.sass';
import scssStyle2 from './normal_css_modules/style.module.scss';

console.log('CSS styles2:', styles2);
console.log('LESS styles2:', lessStyle2);
console.log('SASS styles2:', sassStyle2);
console.log('SCSS styles2:', scssStyle2);