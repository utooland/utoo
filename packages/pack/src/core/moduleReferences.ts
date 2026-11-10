import * as binding from "../binding";

// Query the loaded native binary, not the JS package version. A linked wrapper
// can otherwise advertise support while an older native binary ignores config.
export const moduleReferencesVersion =
  "getModuleReferencesVersion" in binding &&
  typeof binding.getModuleReferencesVersion === "function"
    ? binding.getModuleReferencesVersion()
    : 0;
