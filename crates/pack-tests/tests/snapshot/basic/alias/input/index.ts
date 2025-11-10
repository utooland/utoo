import { foo } from 'hello-a';
import { aliasPkg, aliasA } from "alias-pkg";
import { bar } from '@@/a';
import { aliasB } from '@@/b';
import { a as browserslistA } from 'browserslist';

console.log(browserslistA, foo, aliasPkg, bar);

console.log('a from alias-pkg', aliasA);
console.log('b from alias-pkg', aliasB);
