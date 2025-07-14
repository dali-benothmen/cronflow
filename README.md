# Node-Cronflow

A sophisticated workflow automation engine built on **Node.js + Rust** that combines the developer-friendly experience of Node.js with the rock-solid reliability and performance of Rust.

## Architecture

Node-cronflow follows a clear separation of concerns:

- **Node.js (The SDK)**: Handles the **Developer Experience (DX)**. It's the friendly, flexible, and dynamic "frontend" for the developer.
- **Rust (The Core Engine)**: Handles **Reliability and Performance**. It's the powerful, durable, and stateful "backend" that does the heavy lifting.

## Project Structure

```
node-cronflow/
├── packages/
│   ├── core/          # Rust engine (state management, job execution)
│   ├── sdk/           # Node.js SDK (workflow definition, developer API)
│   └── services/      # Built-in service integrations
├── examples/          # Example workflows
├── docs/             # Documentation
└── scripts/          # Build and deployment scripts
```

## Development Setup

This project uses **Nx monorepo** for optimal Node.js + Rust development.

### Prerequisites

- Node.js 18+
- Rust 1.70+
- Nx CLI (installed globally)

### Quick Start

```bash
# Install dependencies
npm install

# Build all packages
npm run build

# Run tests
npm run test

# Start development mode
npm run dev

# View project graph
npm run graph
```

### Nx Commands

```bash
# Build all packages
nx run-many --target=build

# Build only affected packages
nx affected:build

# Test all packages
nx run-many --target=test

# View dependency graph
nx graph
```

## Development Philosophy

- **Micro-Tasks**: Each development task is a single, focused action
- **Progressive Dependencies**: Each task builds on the previous one
- **AI-Friendly**: Tasks are small enough that an AI can understand the current state
- **Clear Context**: Each task includes the current state and what needs to be done next
- **Monorepo Structure**: Using Nx for optimal Node.js + Rust development

## Key Features

- **Hybrid Architecture**: Node.js for DX, Rust for performance
- **Workflow Definition**: Fluent API for defining complex workflows
- **Service Integration**: Built-in integrations for popular services
- **Testing Harness**: Comprehensive testing framework
- **Production Ready**: Scalable from development to production

## License

MIT
