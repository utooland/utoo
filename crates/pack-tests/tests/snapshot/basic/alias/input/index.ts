import { foo } from 'hello-a';
import { aliasPkg, aliasA } from "alias-pkg";
import { bar } from '@@/a';
import { aliasB } from '@@/b';
import { a as browserslistA } from 'browserslist';
import style from '@/index.less';
import { a } from '@/a?query';
import { output } from '../../../output';

output;

console.log('a', a);

console.log('style', style);

console.log(browserslistA, foo, aliasPkg, bar);

console.log('a from alias-pkg', aliasA);
console.log('b from alias-pkg', aliasB);
