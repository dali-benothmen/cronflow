## 📋 Pull Request

<!--
Please fill out the sections below to help us review your pull request.
-->

### 📝 Description

<!--
Provide a clear and concise description of what this pull request does.
-->

**What does this PR do?**

-
-
- **Why is this change needed?**

<!--
Explain why this change is necessary and what problem it solves.
-->

### 🏷️ Type of Change

<!--
Please delete options that are not relevant.
-->

- [ ] 🐛 Bug fix (non-breaking change which fixes an issue)
- [ ] ✨ New feature (non-breaking change which adds functionality)
- [ ] 💥 Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] 📚 Documentation update
- [ ] 🎨 Style/formatting changes
- [ ] ♻️ Code refactoring (no functional changes)
- [ ] ⚡ Performance improvements
- [ ] 🧪 Test additions or updates
- [ ] 🔧 Build system or dependency changes
- [ ] 🚀 CI/CD improvements

### 🔗 Related Issues

<!--
Link any related issues here. Use "Closes #123" or "Fixes #123" to automatically close issues.
-->

Closes #
Related to #

### 🧪 Testing

<!--
Describe the tests that you ran to verify your changes. Provide instructions so we can reproduce.
-->

**Test Coverage:**

- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Performance tests added/updated
- [ ] Manual testing completed

**Test Instructions:**

```bash
# Commands to run tests
bun test

# Additional test commands if needed
bun run test:package
bun run benchmark
```

**Test Results:**

<!--
Include test output or screenshots if applicable.
-->

### 📊 Performance Impact

<!--
Describe any performance implications of this change.
-->

- [ ] ⚡ Performance improvement
- [ ] 📉 Performance regression
- [ ] ➡️ No performance impact
- [ ] 📈 Performance neutral

**Benchmark Results (if applicable):**

```bash
# Before
# After
```

### 🔍 Code Quality

<!--
Confirm that your code follows our standards.
-->

- [ ] ✅ Code follows TypeScript guidelines
- [ ] ✅ Code follows Rust guidelines (if applicable)
- [ ] ✅ Linting passes: `bun run lint`
- [ ] ✅ Formatting passes: `bun run format:check`
- [ ] ✅ Build succeeds: `bun run build:all`
- [ ] ✅ Tests pass: `bun test`

### 📚 Documentation

<!--
Documentation updates that accompany this change.
-->

- [ ] 📖 README updated
- [ ] 📖 API documentation updated
- [ ] 📖 Examples updated
- [ ] 📖 CONTRIBUTING.md updated (if needed)
- [ ] 📖 No documentation changes needed

### 🚨 Breaking Changes

<!--
If this PR contains breaking changes, please describe them here.
-->

**Breaking Changes:**

-
-
- **Migration Guide:**

<!--
Provide instructions for users to migrate from the old behavior to the new behavior.
-->

### 🔧 Build & Dependencies

<!--
Any changes to build system, dependencies, or configuration.
-->

- [ ] 📦 New dependencies added
- [ ] 📦 Dependencies updated
- [ ] 🔧 Build configuration changed
- [ ] 🔧 CI/CD configuration changed
- [ ] ➡️ No build/dependency changes

### 📋 Checklist

<!--
Go over all the following points, and put an `x` in all the boxes that apply.
-->

#### Before Submitting

- [ ] 🔍 I have searched for existing issues and pull requests
- [ ] 📝 I have updated the documentation accordingly
- [ ] 🧪 I have added tests that prove my fix is effective or that my feature works
- [ ] ✅ All new and existing tests pass
- [ ] 🎯 My code follows the project's style guidelines
- [ ] 🔧 I have performed a self-review of my own code
- [ ] 📊 I have commented my code, particularly in hard-to-understand areas
- [ ] 📝 I have made corresponding changes to the documentation
- [ ] 🚨 My changes generate no new warnings
- [ ] 🧹 I have added comments to my code for any complex logic

#### For New Features

- [ ] 🎯 I have provided a clear use case for the new feature
- [ ] 📖 I have updated the documentation to reflect the new feature
- [ ] 🧪 I have added comprehensive tests for the new feature
- [ ] 📊 I have considered the performance impact of the new feature
- [ ] 🔒 I have considered security implications of the new feature

#### For Bug Fixes

- [ ] 🐛 I have clearly described the bug that was fixed
- [ ] 🧪 I have added tests that would fail without the fix
- [ ] 📖 I have updated documentation if the fix changes behavior
- [ ] 🔍 I have verified the fix works in the described scenarios

### 📸 Screenshots (if applicable)

<!--
Add screenshots or GIFs to help explain your changes.
-->

### 🔍 Additional Context

<!--
Add any other context about the pull request here.
-->

### 📝 Commit Message

<!--
Please provide the commit message that will be used for this PR.
Follow conventional commits format: https://www.conventionalcommits.org/
-->

**Commit Message:**

```
type(scope): description

[optional body]

[optional footer]
```

**Examples:**

- `feat(workflow): add new webhook trigger`
- `fix(core): resolve memory leak in workflow execution`
- `docs(readme): update installation instructions`
- `perf(sdk): optimize workflow definition parsing`

---

## 🎉 Thank You!

Thank you for contributing to Cronflow! Your contributions help make workflow automation faster and more accessible for developers worldwide.

<!--
Please note that by submitting this pull request, you agree that your contributions will be licensed under the MIT License.
-->
