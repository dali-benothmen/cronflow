# Node-Cronflow Tests

This directory contains various test suites for the Node-Cronflow project.

## Directory Structure

```
tests/
├── README.md           # This file
├── napi/              # N-API bridge tests
│   └── bridge-test.js # Node.js tests for Rust N-API functions
└── ...                # Future test directories
```

## N-API Bridge Tests

The `napi/` directory contains tests for the Rust N-API bridge that enables Node.js to call Rust functions.

### Running N-API Tests

1. **Build the Rust core:**

   ```bash
   cd core
   cargo build --release
   ```

2. **Run the N-API bridge test:**
   ```bash
   node tests/napi/bridge-test.js
   ```

### Test Coverage

The N-API bridge tests verify:

- ✅ **Workflow Registration**: Register workflow definitions from Node.js
- ✅ **Run Creation**: Create workflow executions with payloads
- ✅ **Status Retrieval**: Get run status and metadata
- ✅ **Step Execution**: Execute workflow steps

### Expected Results

When all tests pass, you should see:

```
🧪 Testing N-API Bridge...

1️⃣ Testing workflow registration...
✅ Workflow registration successful

2️⃣ Testing run creation...
✅ Run creation successful

3️⃣ Testing run status retrieval...
✅ Status retrieval successful

4️⃣ Testing step execution...
✅ Step execution successful

🎉 All N-API tests passed!
Expected Result: Node.js can call Rust functions via N-API - ✅ VERIFIED

📋 Test Summary:
   • Workflow registration: ✅
   • Run creation: ✅
   • Status retrieval: ✅
   • Step execution: ✅

🚀 N-API Bridge is fully functional!
```

## Test Database

Tests create temporary SQLite databases in the current directory:

- `test_napi_bridge.db` - N-API bridge tests

These files are automatically cleaned up after tests complete.

## Adding New Tests

When adding new test suites:

1. Create a new directory under `tests/`
2. Add a descriptive README for the test suite
3. Update this main README with the new test information
4. Ensure tests clean up after themselves
