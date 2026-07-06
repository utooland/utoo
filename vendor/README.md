# Vendor

This directory contains scripts and templates for publishing npm packages.

## Package Structure

We publish two types of packages for each binary:

1. Main Package (`utoo`)
   - Platform-independent entry package
   - Declares the platform binary packages as `optionalDependencies` so npm
     installs the matching one for the host
   - Ships a Node postinstall (`node ./postinstall.js`) that copies the binary
     into place, and a Node placeholder bin that self-heals on first run if the
     postinstall was skipped
   - Serves as the main entry point for users

2. Binary Package (`@utoo/<binary-name>-<os>-<cpu>`)
   - Platform-specific package containing the actual binary
   - Published for each supported platform (darwin-x64, darwin-arm64,
     linux-x64, linux-arm64, win32-x64)
   - Installed as an optional dependency and copied into place by the main
     package's postinstall

## Publishing Process

The publishing process is automated through GitHub Actions:

1. Binary Packages:
   - Builds binaries for each platform (darwin-x64, darwin-arm64, linux-x64)
   - Uses `npm-binary.sh` to package and publish platform-specific binaries
   - Each binary package contains the actual executable

2. Main Package:
   - Published after all binary packages are available
   - Uses `npm-utoo.sh` to create the entry package
   - Contains a Node postinstall that:
     - Detects the user's platform (`process.platform` / `process.arch`)
     - Copies the matching optional-dependency binary into place
       (Windows: drops `utoo.exe` into the npm prefix and removes npm's shims)
     - Leaves a self-healing placeholder if the optional dep is missing

   The postinstall and placeholder are Node (not `sh`): npm runs lifecycle
   scripts through `cmd.exe` on Windows, where `sh` is not on PATH for a stock
   Node install, so a shell-based postinstall broke `npm i -g utoo` there.

## Supported Platforms

- macOS (x64, arm64)
- Linux (x64, arm64)
- Windows (x64; arm64 falls back to the x64 binary under emulation)

## Scripts

- `npm-binary.sh`: Packages and publishes platform-specific binaries
- `npm-utoo.sh`: Creates and publishes the main `utoo` entry package

## Templates

- `binary.package.json.template`: Template for binary package configuration
- `utoo.package.json.template`: Template for the main `utoo` package
- `postinstall.utoo.js.template`: Node postinstall (fast path)
- `placeholder.utoo.js.template`: Node placeholder bin (self-heal on first run)
- `utx.utoo.js.template`: `utx` launcher that runs `utoo x`
- `postinstall.sh.template`: legacy shell installer (generic binary packages)
