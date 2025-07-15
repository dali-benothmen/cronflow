# Development Setup with Bun

This guide covers setting up the Node-Cronflow development environment using Bun for all development operations.

## Prerequisites

### Required Software

1. **Bun** (>= 1.0.0)
   ```bash
   # Install Bun
   curl -fsSL https://bun.sh/install | bash
   
   # Verify installation
   bun --version
   ```

2. **Rust** (>= 1.70.0)
   ```bash
   # Install Rust
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # Verify installation
   rustc --version
   cargo --version
   ```

3. **Git**
   ```bash
   # Install Git (Ubuntu/Debian)
   sudo apt update && sudo apt install git
   
   # Verify installation
   git --version
   ```

## Development Environment Setup

### 1. Clone the Repository

```bash
git clone https://github.com/node-cronflow/node-cronflow.git
cd node-cronflow
```

### 2. Install Dependencies

```bash
# Install all dependencies with Bun
bun install

# Install only dev dependencies
bun install --dev
```

### 3. Build the Project

```bash
# Build everything (Rust core + TypeScript)
bun run build:all

# Build only TypeScript components
bun run build

# Build only Rust core
bun run build:core
```

### 4. Run Tests

```bash
# Run all tests
bun test

# Run tests in watch mode
bun run dev:test

# Run specific test file
bun test sdk/index.test.ts

# Run with coverage
bun run test:coverage
```

## Development Workflow

### Daily Development Commands

```bash
# Start development server with hot reload
bun run dev

# Watch and rebuild on changes
bun run dev:build

# Run tests in watch mode
bun run dev:test

# Lint code
bun run lint

# Fix linting issues
bun run lint:fix

# Format code
bun run format

# Check formatting
bun run format:check
```

### Package Management

```bash
# Add a dependency
bun add package-name

# Add a dev dependency
bun add --dev package-name

# Remove a dependency
bun remove package-name

# Update dependencies
bun update

# Check for outdated packages
bun outdated

# Audit dependencies
bun audit
```

### Building and Testing

```bash
# Full build (Rust + TypeScript)
bun run build:all

# TypeScript only
bun run build

# Rust core only
bun run build:core

# Debug build
bun run build:core:debug

# Run all tests
bun test

# Run specific tests
bun run test:napi
bun run test:package

# Run benchmarks
bun run benchmark
```

### Cleaning

```bash
# Clean build artifacts
bun run clean

# Clean everything including lock files
bun run clean:all
```

## IDE Setup

### VS Code

1. **Install Extensions**:
   - TypeScript and JavaScript Language Features
   - Rust Analyzer
   - Prettier - Code formatter
   - ESLint

2. **Recommended Settings**:
   ```json
   {
     "typescript.preferences.importModuleSpecifier": "relative",
     "editor.formatOnSave": true,
     "editor.defaultFormatter": "esbenp.prettier-vscode",
     "rust-analyzer.checkOnSave.command": "clippy"
   }
   ```

### Other IDEs

- **IntelliJ IDEA**: Install Rust and TypeScript plugins
- **Vim/Neovim**: Use rust-analyzer and typescript-language-server
- **Emacs**: Use rust-mode and typescript-mode

## Debugging

### TypeScript Debugging

```bash
# Run with debug logging
DEBUG=* bun run dev

# Run tests with verbose output
bun test --verbose
```

### Rust Debugging

```bash
# Build with debug symbols
bun run build:core:debug

# Run with Rust logging
RUST_LOG=debug bun run test:napi
```

### Performance Profiling

```bash
# Run benchmarks
bun run benchmark

# Compare with Node.js
bun run benchmark:node
```

## Troubleshooting

### Common Issues

1. **Bun not found**
   ```bash
   # Reinstall Bun
   curl -fsSL https://bun.sh/install | bash
   source ~/.bashrc
   ```

2. **Rust toolchain issues**
   ```bash
   # Update Rust
   rustup update
   
   # Install specific toolchain
   rustup install stable
   rustup default stable
   ```

3. **Native addon build failures**
   ```bash
   # Clean and rebuild
   bun run clean:all
   bun install
   bun run build:all
   ```

4. **Permission issues**
   ```bash
   # Fix permissions
   sudo chown -R $USER:$USER .
   chmod +x core/core.node
   ```

### Performance Issues

1. **Slow builds**: Use `bun run build:core:debug` for development
2. **Memory issues**: Check for memory leaks in Rust code
3. **Test failures**: Ensure all dependencies are installed

## Production Deployment

### Building for Production

```bash
# Build optimized version
bun run build:all

# Verify build artifacts
ls -la dist/
ls -la core/core.node
```

### Publishing

```bash
# Dry run release
bun run release:dry-run

# Create release
bun run release
```

**Note**: Publishing still uses npm for registry compatibility.

## Environment Variables

### Development

```bash
# Enable debug logging
export DEBUG=*
export RUST_LOG=debug

# Set database path
export CRONFLOW_DB_PATH=./dev.db
```

### Production

```bash
# Set production database
export CRONFLOW_DB_PATH=/var/lib/cronflow/prod.db

# Set log level
export RUST_LOG=info
```

## Contributing

### Development Workflow

1. **Create feature branch**
   ```bash
   git checkout -b feature/your-feature
   ```

2. **Make changes**
   ```bash
   # Edit code
   bun run dev:test  # Run tests in watch mode
   ```

3. **Test changes**
   ```bash
   bun run build:all
   bun test
   bun run test:napi
   ```

4. **Commit changes**
   ```bash
   git add .
   git commit -m "feat: your feature description"
   ```

5. **Push and create PR**
   ```bash
   git push origin feature/your-feature
   ```

### Code Quality

- **Linting**: `bun run lint`
- **Formatting**: `bun run format`
- **Testing**: `bun test`
- **Benchmarks**: `bun run benchmark`

## Summary

The Bun development workflow provides:
- ✅ **Faster builds** with Bun's optimized bundler
- ✅ **Native TypeScript support** (no build step needed)
- ✅ **Fast test execution** with Bun's test runner
- ✅ **Simplified package management** with Bun
- ✅ **Excellent development experience** with hot reloading
- ✅ **Production-ready** with optimized builds 