# 🤝 Contributing to Cronflow

Thank you for your interest in contributing to Cronflow! This document provides guidelines and information for contributors.

## 📋 Table of Contents

- [🚀 Quick Start](#-quick-start)
- [🔧 Development Setup](#-development-setup)
- [📝 Contribution Guidelines](#-contribution-guidelines)
- [🏗️ Project Structure](#️-project-structure)
- [🧪 Testing](#-testing)
- [📦 Building](#-building)
- [🎯 Code Style](#-code-style)
- [📋 Pull Request Process](#-pull-request-process)
- [🐛 Bug Reports](#-bug-reports)
- [💡 Feature Requests](#-feature-requests)
- [📄 License](#-license)

---

## 🚀 Quick Start

1. **Fork** the repository
2. **Clone** your fork: `git clone https://github.com/YOUR_USERNAME/node-cronflow.git`
3. **Install dependencies**: `bun install`
4. **Build the project**: `bun run build:all`
5. **Run tests**: `bun test`
6. **Create a branch**: `git checkout -b feature/your-feature-name`
7. **Make your changes** and commit them
8. **Push** to your fork and create a Pull Request

---

## 🔧 Development Setup

### Prerequisites

- **Node.js** >= 18.0.0
- **Bun** >= 1.0.0
- **Rust** (for core components)
- **Git**

### Installation

```bash
# Clone the repository
git clone https://github.com/node-cronflow/node-cronflow.git
cd node-cronflow

# Install dependencies
bun install

# Build all components
bun run build:all

# Run tests to verify setup
bun test
```

### Development Commands

```bash
# Development with hot reload
bun run dev

# Run tests in watch mode
bun run dev:test

# Build in watch mode
bun run dev:build

# Lint code
bun run lint

# Fix linting issues
bun run lint:fix

# Format code
bun run format

# Check formatting
bun run format:check

# Run benchmarks
bun run benchmark

# Run examples
bun run examples
```

---

## 📝 Contribution Guidelines

### Before You Start

1. **Check existing issues** - Your idea might already be discussed
2. **Discuss major changes** - Open an issue for significant features
3. **Follow the coding standards** - See [Code Style](#-code-style) section
4. **Write tests** - All new features should include tests
5. **Update documentation** - Keep docs in sync with code changes

### What We're Looking For

- **Bug fixes** - Help us squash bugs!
- **Performance improvements** - Cronflow is all about speed
- **New features** - Especially workflow triggers and actions
- **Documentation** - Improve our docs and examples
- **Tests** - Increase our test coverage
- **Examples** - Show how to use Cronflow effectively

### What We're NOT Looking For

- Breaking changes without discussion
- Changes that don't align with our performance goals
- Features that add unnecessary complexity
- Changes that don't include tests

---

## 🏗️ Project Structure

```
node-cronflow/
├── core/                 # Rust core components
├── src/                  # Main TypeScript source
├── sdk/                  # SDK components
├── services/             # Service layer
├── tests/                # Test files
├── examples/             # Example workflows
├── docs/                 # Documentation
├── scripts/              # Build and utility scripts
├── benchmarks/           # Performance benchmarks
└── .github/              # GitHub workflows and templates
```

### Key Components

- **`core/`** - Rust-based performance-critical components
- **`src/`** - Main TypeScript application code
- **`sdk/`** - Public SDK for workflow definitions
- **`services/`** - Service layer for external integrations
- **`examples/`** - Example workflows and use cases

---

## 🧪 Testing

### Running Tests

```bash
# Run all tests
bun test

# Run tests once (no watch mode)
bun test --run

# Run tests with UI
bun test --ui

# Run tests with coverage
bun test --coverage

# Test package build
bun run test:package

# Test NAPI bridge
bun run test:napi
```

### Writing Tests

- **Unit tests** - Test individual functions and classes
- **Integration tests** - Test workflow execution
- **Performance tests** - Ensure we maintain speed
- **Example tests** - Verify examples work correctly

### Test Structure

```typescript
// Example test structure
describe('Workflow Definition', () => {
  it('should create a simple workflow', () => {
    const workflow = cronflow.define({
      id: 'test-workflow',
      name: 'Test Workflow',
    });

    expect(workflow).toBeDefined();
  });
});
```

---

## 📦 Building

### Build Commands

```bash
# Build everything
bun run build

# Build individual components
bun run build:sdk
bun run build:services
bun run build:main

# Build Rust core
bun run build:core

# Build with debug symbols
bun run build:core:debug

# Generate TypeScript types
bun run generate:types
```

### Build Process

1. **Rust Core** - Builds the performance-critical components
2. **TypeScript SDK** - Builds the public API
3. **Services** - Builds the service layer
4. **Main App** - Builds the main application
5. **Type Generation** - Generates TypeScript definitions

---

## 🎯 Code Style

### TypeScript Guidelines

- **Use TypeScript** - All new code should be in TypeScript
- **Strict mode** - We use strict TypeScript settings
- **Type everything** - Avoid `any` types
- **Use interfaces** - Define clear contracts
- **Async/await** - Prefer async/await over Promises

### Rust Guidelines

- **Follow Rust conventions** - Use `rustfmt` and `clippy`
- **Performance first** - Optimize for speed
- **Memory safety** - Leverage Rust's safety features
- **Documentation** - Document public APIs

### General Guidelines

- **Consistent naming** - Use camelCase for variables, PascalCase for classes
- **Clear comments** - Comment complex logic
- **Small functions** - Keep functions focused and small
- **Error handling** - Handle errors gracefully
- **Performance** - Consider performance implications

### Linting and Formatting

```bash
# Lint code
bun run lint

# Fix linting issues
bun run lint:fix

# Format code
bun run format

# Check formatting
bun run format:check
```

---

## 📋 Pull Request Process

### Before Submitting

1. **Ensure tests pass** - `bun test`
2. **Check linting** - `bun run lint`
3. **Verify formatting** - `bun run format:check`
4. **Build successfully** - `bun run build:all`
5. **Update documentation** - If needed
6. **Add examples** - For new features

### Pull Request Template

We use a pull request template (see `.github/pull_request_template.md`) that includes:

- **Description** - What the PR does
- **Type of change** - Bug fix, feature, etc.
- **Testing** - How to test the changes
- **Breaking changes** - Any breaking changes
- **Performance impact** - Performance implications

### Review Process

1. **Automated checks** - CI/CD pipeline runs
2. **Code review** - At least one maintainer review
3. **Testing** - Manual testing may be required
4. **Documentation** - Ensure docs are updated
5. **Merge** - Once approved and tests pass

---

## 🐛 Bug Reports

### Before Reporting

1. **Check existing issues** - Search for similar reports
2. **Try the latest version** - Ensure you're using the latest release
3. **Reproduce the issue** - Create a minimal reproduction
4. **Check documentation** - The issue might be documented

### Bug Report Template

```markdown
**Describe the bug**
A clear description of what the bug is.

**To Reproduce**
Steps to reproduce the behavior:

1. Go to '...'
2. Click on '....'
3. See error

**Expected behavior**
A clear description of what you expected to happen.

**Environment:**

- OS: [e.g. macOS, Windows, Linux]
- Node.js version: [e.g. 18.0.0]
- Bun version: [e.g. 1.0.0]
- Cronflow version: [e.g. 0.0.0]

**Additional context**
Add any other context about the problem here.
```

---

## 💡 Feature Requests

### Before Requesting

1. **Check existing features** - The feature might already exist
2. **Search issues** - Similar requests might be discussed
3. **Consider alternatives** - There might be a better approach
4. **Think about performance** - How does it affect speed?

### Feature Request Template

```markdown
**Is your feature request related to a problem?**
A clear description of what the problem is.

**Describe the solution you'd like**
A clear description of what you want to happen.

**Describe alternatives you've considered**
A clear description of any alternative solutions.

**Additional context**
Add any other context or screenshots about the feature request.
```

---

## 📄 License

By contributing to Cronflow, you agree that your contributions will be licensed under the MIT License.

---

## 🎉 Thank You!

Thank you for contributing to Cronflow! Your contributions help make workflow automation faster and more accessible for developers worldwide.

If you have any questions about contributing, feel free to:

- Open an issue for discussion
- Join our community discussions
- Reach out to the maintainers

Happy coding! 🚀
