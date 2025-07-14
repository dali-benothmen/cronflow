# Division of Responsibilities: Node.js vs. Rust

## Core Philosophy

The architecture follows a clear separation of concerns:

- **Node.js (The SDK)**: Handles the **Developer Experience (DX)**. It's the friendly, flexible, and dynamic "frontend" for the developer.
- **Rust (The Core Engine)**: Handles **Reliability and Performance**. It's the powerful, durable, and stateful "backend" that does the heavy lifting.

## Detailed Responsibility Breakdown

| Responsibility / Domain            | Node.js (The SDK)                                                                                                                                                                                                                      | Rust (The Core Engine)                                                                                                                                                                                                                                 |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Workflow Definition**            | **PRIMARY**. Provides the entire fluent API: `cronflow.define()`, `.step()`, `.if()`, `.retry()`, etc. Its job is to build a declarative JSON representation (WDO) of the workflow.                                                    | **SECONDARY**. Receives the final JSON WDO. Its only job is to parse this JSON into its internal Rust structs for storage and execution. It has no knowledge of how the JSON was built.                                                                |
| **State Management & Persistence** | **STATELESS**. It holds no state between steps. It receives a `ctx` object for each job, uses it, and then forgets it.                                                                                                                 | **PRIMARY**. The "brain" of the system. Manages the state of every workflow and step (`RUNNING`, `FAILED`, etc.). It owns the database connection and is responsible for all CRUD operations on the internal SQLite/Postgres database.                 |
| **Scheduling & Triggers**          | **DEFINITION**. Defines what the trigger is (`.onSchedule(...)`, `.onWebhook(...)`). This information is serialized into the WDO.                                                                                                      | **IMPLEMENTATION**. Runs the actual cron scheduler. Runs the actual web server to listen for webhooks. Manages the stateful logic for polling. It is the active "listener" for all events.                                                             |
| **Task Execution**                 | **PRIMARY**. This is where the user's business logic runs. The "Task Runner" module receives a job from Rust, finds the correct JavaScript function `(ctx) => ...`, and executes it (e.g., fetch calls, db queries).                   | **SECONDARY**. Acts as a "Dispatcher." It tells the Node.js Task Runner which step to execute and then waits for a result. It treats the Node.js side as a "function execution service."                                                               |
| **Integrations & Services**        | **PRIMARY**. `defineService` and `.withConfig` are pure Node.js concepts. All the logic for talking to external APIs (Stripe, Slack, etc.) is written in TypeScript and lives here.                                                    | **SECONDARY**. Knows nothing about specific services like Slack or JIRA. It only provides the generic, low-level primitives that integrations can use (e.g., `engine.storage`, `engine.createWebhookTrigger`).                                         |
| **Error Handling & Retry Logic**   | **PRODUCES ERRORS**. When a user's step throws an exception, the Node.js Task Runner catches it and passes the serialized error back to Rust.                                                                                          | **MANAGES RETRIES**. Receives the error from Node.js. It then reads the step's retry configuration, manages the backoff delay, updates the attempt count in the database, and decides whether to re-dispatch the job or mark it as permanently failed. |
| **Concurrency & Performance**      | **SECONDARY**. The Node.js event loop handles I/O concurrency for the tasks it is told to run.                                                                                                                                         | **PRIMARY**. The tokio multi-threaded runtime manages the engine's worker pool for high-throughput job dispatching. It's responsible for connection pooling (DB, HTTP) and keeping overall CPU/memory usage low.                                       |
| **Testing**                        | **PRIMARY**. Provides the entire `.test()` harness (`.mockStep`, `.expectAction`, etc.). It includes an in-memory workflow runner that simulates the Rust engine's behavior to enable fast, easy testing without any Rust interaction. | **NOT INVOLVED** in the user-facing testing API. The Rust core's correctness is verified by its own separate suite of Rust unit and integration tests (`cargo test`).                                                                                  |
| **Configuration & Lifecycle**      | **PRIMARY**. Provides the user-facing API to start and stop the system (`cronflow.start()`, `cronflow.stop()`). It's also where the user provides `.env` or other configuration.                                                       | **IMPLEMENTATION**. Implements the actual lifecycle. It receives the start command and boots up all its internal components (scheduler, web server, DB pool). On stop, it gracefully shuts them down.                                                  |
| **Logging**                        | **SECONDARY**. The user's code can `console.log` within a step. The SDK can also provide a structured logger on the `ctx` object.                                                                                                      | **PRIMARY**. The engine performs its own structured logging for all core events (e.g., "Run Started", "Dispatching Job", "State Updated", "Engine Shutdown"). This provides a complete audit trail of the engine's internal operations.                |

## Summary in Simple Terms

### **You write your workflow in Node.js.**

Node.js is responsible for:

- ✅ The beautiful API
- ✅ The integrations
- ✅ The testing framework
- ✅ Running your actual business logic inside each step

### **Rust makes sure it runs perfectly.**

Rust is responsible for:

- ✅ Scheduling it at the right time
- ✅ Remembering what step to run next
- ✅ Surviving crashes
- ✅ Retrying failures
- ✅ Doing all of this with incredible speed and minimal memory usage

## Key Benefits of This Division

### **Node.js Strengths Leveraged:**

- **Developer Experience**: Familiar syntax, rich ecosystem, TypeScript support
- **Integration Ecosystem**: Largest package ecosystem for APIs and services
- **Flexibility**: Dynamic nature perfect for user-defined business logic
- **Testing**: Easy mocking and in-memory execution

### **Rust Strengths Leveraged:**

- **Performance**: Zero-cost abstractions, minimal memory usage
- **Reliability**: Memory safety, thread safety, crash resistance
- **State Management**: Efficient, durable state persistence
- **Concurrency**: High-performance async runtime with tokio

### **Clear Boundaries:**

- **No Overlap**: Each language handles what it does best
- **Clean Interfaces**: N-API bridge provides clear communication
- **Independent Evolution**: Each side can evolve independently
- **Fault Isolation**: Issues in one side don't affect the other

This architecture provides the best of both worlds: the developer-friendly experience of Node.js with the rock-solid reliability and performance of Rust.
