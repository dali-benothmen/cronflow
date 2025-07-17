# Node-Cronflow Examples

This folder contains comprehensive examples demonstrating the various features of the Node-Cronflow SDK.

## 📁 Examples Overview

### 1. Simple Webhook (`01-simple-webhook.ts`)

- **Purpose**: Basic webhook workflow
- **Features**: Webhook trigger, step and action methods
- **Run**: `bun run examples/run-simple-webhook.ts`

### 2. Scheduled Workflow (`02-scheduled-workflow.ts`)

- **Purpose**: Time-based workflow execution
- **Features**: CRON scheduling, multi-step workflow
- **Run**: `bun run examples/run-scheduled-workflow.ts`

### 3. Robust Workflow (`03-robust-workflow.ts`)

- **Purpose**: Error handling and reliability
- **Features**: Retry logic, timeout configuration, exponential backoff
- **Run**: `bun run examples/run-robust-workflow.ts`

### 4. Complex Workflow (`04-complex-workflow.ts`)

- **Purpose**: Advanced workflow features
- **Features**: Multiple triggers, caching, validation, logging
- **Run**: `bun run examples/run-complex-workflow.ts`

### 5. Control Flow Workflow (`05-control-flow-workflow.ts`)

- **Purpose**: Advanced control flow methods
- **Features**: Conditional logic, parallel execution, race conditions, loops
- **Run**: `bun run examples/run-control-flow-workflow.ts`

## 🚀 Running Examples

### Run All Examples

```bash
bun run examples/run-all-examples.ts
```

### Run Individual Examples

```bash
# Simple webhook
bun run examples/run-simple-webhook.ts

# Scheduled workflow
bun run examples/run-scheduled-workflow.ts

# Robust workflow
bun run examples/run-robust-workflow.ts

# Complex workflow
bun run examples/run-complex-workflow.ts

# Control flow workflow
bun run examples/run-control-flow-workflow.ts
```

### Run Specific Example File

```bash
bun run examples/01-simple-webhook.ts
bun run examples/02-scheduled-workflow.ts
bun run examples/03-robust-workflow.ts
bun run examples/04-complex-workflow.ts
bun run examples/05-control-flow-workflow.ts
```

## 📊 Features Demonstrated

### Basic Features

- ✅ Workflow definition and configuration
- ✅ Step and action methods
- ✅ Context object usage
- ✅ Trigger setup (webhook, schedule, manual)

### Advanced Features

- ✅ Retry logic with exponential backoff
- ✅ Timeout configuration
- ✅ Caching with TTL
- ✅ Input validation
- ✅ Logging and notifications

### Control Flow Features

- ✅ Conditional logic (if/elseIf/else/endIf)
- ✅ Parallel execution
- ✅ Race conditions
- ✅ While loops
- ✅ Complex business logic

### Error Handling

- ✅ Input validation
- ✅ Error propagation
- ✅ Fallback mechanisms
- ✅ Graceful degradation

## 🎯 Learning Path

1. **Start with Simple Webhook** - Understand basic concepts
2. **Try Scheduled Workflow** - Learn time-based execution
3. **Explore Robust Workflow** - Understand error handling
4. **Study Complex Workflow** - See advanced features
5. **Master Control Flow** - Learn conditional and parallel execution

## 📝 Notes

- All examples are self-contained and can be run independently
- Each example demonstrates specific SDK features
- Examples include comprehensive logging for learning purposes
- The control flow example is the most comprehensive, showing all advanced features

## 🔧 Customization

Feel free to modify these examples to:

- Add your own business logic
- Test different configurations
- Experiment with new features
- Create your own workflows

## 📚 Next Steps

After running these examples, you can:

1. Create your own workflows
2. Integrate with your existing systems
3. Explore the API documentation
4. Build production-ready workflows
