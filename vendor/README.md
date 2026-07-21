# Vendor

This directory contains scripts and templates for publishing npm packages.

## Package Structure

We publish two types of packages for each binary:

1. Main Package (`utoo`)
   - Platform-independent entry package
   - Declares the platform binary packages as `optionalDependencies` so npm
     installs the matching one for the host
   - Ships immutable Node launchers that resolve the installed platform package
     and spawn its native binary
   - Does not use install lifecycle hooks or modify package-manager-owned files
   - Serves as the main entry point for users

2. Binary Package (`@utoo/<binary-name>-<os>-<cpu>`)
   - Platform-specific package containing the actual binary
   - Published for each supported platform (darwin-x64, darwin-arm64,
     linux-x64, linux-arm64, win32-x64)
   - Installed as an optional dependency and executed in place by the main
     package's launcher
   - Declares `preferUnplugged` so Yarn Plug'n'Play materializes the executable
     outside its zip archive

## Publishing Process

The publishing process is automated through GitHub Actions:

1. Binary Packages:
   - Builds binaries for each platform (darwin-x64, darwin-arm64, linux-x64)
   - Uses `npm-binary.sh` to package and publish platform-specific binaries
   - Each binary package contains the actual executable

2. Main Package:
   - Published after all binary packages are available
   - Uses `npm-utoo.sh` to create the entry package
   - Contains Node launchers that:
     - Detect the user's platform (`process.platform` / `process.arch`)
     - Locate the matching optional package with Node module resolution
     - Spawn its native executable while forwarding arguments, stdio, exit
       status, and termination signals

   Artifact download, platform filtering, integrity verification, and caching
   remain the package manager's responsibility. The launchers never download,
   extract, copy, overwrite, or remove installed files.

## Supported Platforms

- macOS (x64, arm64)
- Linux (x64, arm64)
- Windows (x64; arm64 installs the x64 package and uses system emulation)

## Scripts

- `npm-binary.sh`: Packages and publishes platform-specific binaries
- `npm-utoo.sh`: Creates and publishes the main `utoo` entry package

## Templates

- `binary.package.json.template`: Template for binary package configuration
- `utoo.package.json.template`: Template for the main `utoo` package
- `launcher.utoo.js.template`: shared platform resolver and native process runner
- `utoo.utoo.js.template`: `utoo` / `ut` launcher entry
- `utx.utoo.js.template`: `utx` launcher entry (`utoo x`)
- `postinstall.sh.template`: legacy shell installer (generic binary packages)
