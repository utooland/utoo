import NativeEsm, { count, increment, named } from "native-esm";

increment();
console.log(NativeEsm, named, count);
