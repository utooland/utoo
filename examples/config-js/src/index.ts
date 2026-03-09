declare const GREETING: string;
declare const APP_VERSION: number;

// These are injected at build time via utoopack.config.mjs define
console.log("GREETING:", GREETING);
console.log("APP_VERSION:", APP_VERSION);
