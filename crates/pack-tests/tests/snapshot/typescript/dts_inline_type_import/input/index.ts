import { type MyType, type TypeMetadata as Metadata } from './types';
import { runtimeValue, type RuntimeType } from './runtime';

const value: MyType = 'inline type import';
const metadata: Metadata = { source: '.d.ts' };
const runtime: RuntimeType = runtimeValue;

console.log(value, metadata.source, runtime);
