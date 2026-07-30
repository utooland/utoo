import $ from "jquery";
import value, { named } from "async-value";
import esmValue, { named as esmNamed } from "async-esm-value";
import NativeEsm, { named as nativeEsmNamed } from "native-esm-value";

console.log(
    $.ready,
    value.default,
    value.named,
    named,
    esmValue,
    esmNamed,
    NativeEsm,
    nativeEsmNamed,
);
