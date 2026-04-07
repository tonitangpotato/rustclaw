# TypeScript Sample Fixture

This is a minimal TypeScript project used for testing the LSP integration in gid-core.

## Structure

- `utils.ts` - Defines functions and classes
- `index.ts` - Imports and calls functions from utils.ts
- `package.json` - NPM package configuration
- `tsconfig.json` - TypeScript compiler configuration

## Expected Call Edges

When LSP client analyzes this project, it should detect these precise call edges:

1. `index.ts:main` → `utils.ts:greet`
2. `index.ts:main` → `utils.ts:add`
3. `index.ts:main` → `utils.ts:Calculator.multiply`
4. `index.ts:main` → `utils.ts:Calculator.divide`

## Setup

```bash
cd crates/gid-core/tests/fixtures/typescript-sample
npm install
```

## Manual Testing

You can manually test LSP queries using typescript-language-server:

```bash
# Install if not already installed
npm install -g typescript-language-server typescript

# Start server (in one terminal)
typescript-language-server --stdio

# Send LSP requests (example using JSON-RPC)
# See: https://microsoft.github.io/language-server-protocol/
```
