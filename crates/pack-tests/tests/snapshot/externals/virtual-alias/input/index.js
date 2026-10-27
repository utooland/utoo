import { state as initialized } from "@mvp/request/h5";
import { state as unscoped } from "plain-request/h5";
import { state as requested } from "virtual:request/h5";
import { state as literal } from "virtual:request/[h5]{test}*";

initialized.initialized = true;

export const sameInstance = initialized === requested;
export const requestInitialized = requested.initialized;
export const sameUnscopedInstance = initialized === unscoped;
export const unscopedInitialized = unscoped.initialized;
export const sameLiteralInstance = initialized === literal;
export const literalInitialized = literal.initialized;
