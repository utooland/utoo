import { foo } from 'hello-a';
import { aliasPkg } from "alias-pkg";
import { bar } from '@@/a';

console.log(foo, aliasPkg, bar);

console.log('xxxx');