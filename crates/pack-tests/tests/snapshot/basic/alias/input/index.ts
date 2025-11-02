import { foo } from 'hello-a';
import { aliasPkg, aliasA } from "alias-pkg";
import { bar } from '@@/a.ts';
import { aliasB } from '@@/b';

console.log(foo, aliasPkg, bar);

console.log('a from alias-pkg', aliasA);
console.log('b from alias-pkg', aliasB);
