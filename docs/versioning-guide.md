# Versioning Guide

## Overview

Node-Cronflow uses **Semantic Versioning** with **Conventional Commits** for automatic version management.

## How It Works

### 1. Commit Message Format

All commits must follow the conventional commit format:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types:**
- `feat`: New feature (minor version bump)
- `fix`: Bug fix (patch version bump)
- `docs`: Documentation changes (no version bump)
- `style`: Code style changes (no version bump)
- `refactor`: Code refactoring (no version bump)
- `perf`: Performance improvements (patch version bump)
- `test`: Test changes (no version bump)
- `chore`: Build/tooling changes (no version bump)

**Scopes:**
- `sdk`: Node.js SDK changes
- `core`: Rust core engine changes
- `services`: Built-in services changes
- `docs`: Documentation changes
- `build`: Build system changes

**Examples:**
```bash
git commit -m "feat(sdk): add parallel workflow execution"
git commit -m "fix(core): resolve memory leak in state manager"
git commit -m "docs: update API reference"
git commit -m "chore(build): update TypeScript configuration"
```

### 2. Breaking Changes

For breaking changes, add `!` after the type:

```bash
git commit -m "feat(sdk)!: change workflow API signature"
```

Or use the footer:
```bash
git commit -m "feat: update payment gateway

BREAKING CHANGE: drops support for PayPal v1"
```

### 3. Version Bumps

- **Patch** (`0.0.1`): Bug fixes, performance improvements
- **Minor** (`0.1.0`): New features (backward compatible)
- **Major** (`1.0.0`): Breaking changes

## Development Workflow

### 1. Making Changes

1. Create a feature branch:
   ```bash
   git checkout -b feat/add-webhook-support
   ```

2. Make your changes and commit using conventional format:
   ```bash
   git add .
   git commit -m "feat(sdk): add webhook trigger support"
   ```

3. Push and create a pull request:
   ```bash
   git push origin feat/add-webhook-support
   ```

### 2. Releasing

**Automatic Release (Recommended):**
```bash
# Merge to main branch
git checkout main
git merge feat/add-webhook-support

# Run semantic-release
npm run release
```

**Manual Release (if needed):**
```bash
# Dry run to see what would be released
npm run release:dry-run

# Actual release
npm run release
```

### 3. What Happens During Release

1. **Analyzes commits** since last release
2. **Determines version bump** based on commit types
3. **Updates version** in package.json
4. **Generates changelog** entries
5. **Creates git tag** with new version
6. **Publishes to npm** (if configured)

## Configuration Files

### `.releaserc.json`
```json
{
  "branches": ["main"],
  "plugins": [
    "@semantic-release/commit-analyzer",
    "@semantic-release/release-notes-generator",
    "@semantic-release/changelog",
    "@semantic-release/npm",
    "@semantic-release/git"
  ],
  "preset": "angular"
}
```

### `.gitmessage`
Template for commit messages with examples.

### `CHANGELOG.md`
Automatically generated changelog file.

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Release
on:
  push:
    branches: [main]

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
        with:
          fetch-depth: 0
      
      - uses: actions/setup-node@v3
        with:
          node-version: '18'
          registry-url: 'https://registry.npmjs.org'
      
      - run: npm ci
      - run: npm run build
      - run: npm run test
      - run: npm run release
        env:
          NODE_AUTH_TOKEN: ${{secrets.NPM_TOKEN}}
          GITHUB_TOKEN: ${{secrets.GITHUB_TOKEN}}
```

## Best Practices

### 1. Commit Messages
- Use present tense ("add" not "added")
- Keep subject line under 50 characters
- Use body for detailed explanations
- Reference issues when relevant

### 2. Branch Strategy
- `main`: Production-ready code
- `develop`: Integration branch
- `feature/*`: New features
- `fix/*`: Bug fixes
- `release/*`: Release preparation

### 3. Release Strategy
- **Pre-releases**: Use `alpha`, `beta`, `rc` suffixes
- **Hotfixes**: Create `fix/*` branches from main
- **Feature flags**: Use feature toggles for gradual rollouts

### 4. Version Management
- Never manually edit version in package.json
- Let semantic-release handle all versioning
- Use `npm run release:dry-run` to preview changes

## Troubleshooting

### Common Issues

1. **No version bump**: Check commit message format
2. **Wrong version bump**: Verify commit type and scope
3. **Release fails**: Check git authentication and npm tokens
4. **Changelog not updated**: Ensure conventional commit format

### Debug Commands

```bash
# Check commit history
git log --oneline

# Preview release
npm run release:dry-run

# Check current version
npm version

# View changelog
cat CHANGELOG.md
```

## Migration from Manual Versioning

If migrating from manual versioning:

1. Set version to `0.0.0` in package.json
2. Create initial commit with `chore: initial commit`
3. Run `npm run release` to create first release
4. Continue with conventional commits

This ensures a clean versioning history from the start. 