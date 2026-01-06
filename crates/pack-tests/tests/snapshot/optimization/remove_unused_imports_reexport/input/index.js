// Entry only imports fnB, but wrapper.js uses fnA
// BUG: fnA factory is removed because entry doesn't use it
import { fnB } from './wrapper';

console.log(fnB());
