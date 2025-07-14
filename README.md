# Node-Cronflow

A sophisticated workflow automation engine built on Node.js + Rust, providing a code-first alternative to tools like n8n.

## 🚀 Quick Start

```bash
npm install node-cronflow
```

```javascript
import { cronflow, defineService } from 'node-cronflow';

// Define a service
const stripeService = defineService('stripe', {
  version: '1.0.0',
  configSchema: z.object({ apiKey: z.string() }),
  createInstance: (config) => ({ charge: (amount) => /* ... */ })
});

// Create a workflow
const workflow = cronflow
  .define('payment-workflow')
  .onWebhook('/webhook/payment')
  .step('validate', (ctx) => {
    // Validate payment data
    return ctx.payload;
  })
  .step('charge', stripeService.charge)
  .step('notify', (ctx) => {
    // Send notification
    return { success: true };
  });

// Start the engine
cronflow.start();
```

## 🏗️ Architecture

Node-Cronflow uses a **hybrid architecture** combining the best of both worlds:

- **Node.js**: Developer experience, fluent API, integrations
- **Rust**: Core engine, state management, reliability
- **N-API**: High-performance communication bridge

## 📦 Installation

```bash
npm install node-cronflow
```

That's it! No additional packages needed - everything is included in one package.

## 🔧 Development

```bash
# Install dependencies
npm install

# Build the project
npm run build

# Run tests
npm test

# Development mode
npm run dev
```

## 📁 Project Structure

```
node-cronflow/
├── core/             # Rust engine (Cargo.toml, src/)
├── sdk/              # Node.js SDK (TypeScript)
├── services/         # Built-in services (TypeScript)
├── src/              # Main entry point (index.ts)
├── examples/         # Example workflows
├── docs/             # Documentation
└── dist/             # Build output
```

## 🎯 Features

- **Code-first workflows** - Define workflows in TypeScript
- **Built-in services** - Stripe, Slack, HTTP, and more
- **High performance** - Rust core engine
- **Type safety** - Full TypeScript support
- **Simple installation** - One package, everything included

## 📚 Documentation

- [API Reference](./docs/api-reference.md)
- [Architecture](./docs/architecture.md)
- [Versioning Guide](./docs/versioning-guide.md)
- [Examples](./examples/)

## 🤝 Contributing

### Development Workflow

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/your-feature`
3. Make your changes following [conventional commits](./docs/versioning-guide.md)
4. Add tests
5. Submit a pull request

### Commit Message Format

We use [Conventional Commits](https://www.conventionalcommits.org/) for versioning:

```bash
git commit -m "feat(sdk): add parallel workflow execution"
git commit -m "fix(core): resolve memory leak in state manager"
git commit -m "docs: update API reference"
```

See [Versioning Guide](./docs/versioning-guide.md) for detailed information.

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.
