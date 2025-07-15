# Node-Cronflow Development Tasks

A granular, step-by-step development plan using a monolith architecture for Node.js + Rust project.

## Development Philosophy

- **Micro-Tasks**: Each task is a single, focused action
- **Progressive Dependencies**: Each task builds on the previous one
- **AI-Friendly**: Tasks are small enough that an AI can understand the current state
- **Clear Context**: Each task includes the current state and what needs to be done next
- **Monolith Structure**: Using a single package with root-level directories for simplicity and reliability

---

## Phase 1: Monolith Foundation

### Task 1.1: Initialize Monolith Project Structure

**Current State**: Empty directory
**Goal**: Set up monolith project with Node.js and Rust components

**Actions**:

- [x] Initialize npm package: `npm init -y`
- [x] Create root-level directory structure
- [x] Add TypeScript configuration
- [x] Add build scripts for Rust core and TypeScript compilation
- [x] Configure npm publishing and validation

**Expected Result**: Monolith project structure is initialized with build system

---

### Task 1.2: Create Project Structure

**Current State**: Basic npm package exists
**Goal**: Set up the monolith directory structure

**Actions**:

- [x] Create `core/` directory for Rust engine
- [x] Create `sdk/` directory for Node.js SDK
- [x] Create `services/` directory for built-in services
- [x] Create `src/` directory for main entry point
- [x] Create `examples/` directory for example workflows
- [x] Create `docs/` directory for documentation
- [x] Create `scripts/` directory for build scripts

**Expected Result**: Monolith directory structure is created

---

### Task 1.3: Initialize Rust Core Package

**Current State**: Directory structure exists
**Goal**: Set up the Rust core package

**Actions**:

- [x] Create `core/Cargo.toml` with dependencies
- [x] Add `node-api` dependency for N-API bindings
- [x] Add `serde` and `serde_json` for JSON handling
- [x] Add `rusqlite` for database operations
- [x] Create basic `core/src/lib.rs`

**Expected Result**: Rust core package is initialized and builds successfully

---

### Task 1.4: Initialize Node.js SDK Package

**Current State**: Rust core package exists
**Goal**: Set up the Node.js SDK package

**Actions**:

- [x] Create `sdk/package.json` with dependencies
- [x] Add TypeScript configuration for SDK
- [x] Add `@types/node` for Node.js types
- [x] Add `zod` for schema validation
- [x] Create basic `sdk/src/index.ts`

**Expected Result**: Node.js SDK package is initialized and builds successfully

---

### Task 1.5: Initialize Services Package

**Current State**: SDK package exists
**Goal**: Set up the services package for built-in integrations

**Actions**:

- [x] Create `services/package.json` with dependencies
- [x] Add TypeScript configuration for services
- [x] Create basic service exports
- [x] Add service template structure

**Expected Result**: Services package is initialized and ready for service templates

---

### Task 1.6: Configure Build System

**Current State**: All packages exist
**Goal**: Set up build scripts and dependencies

**Actions**:

- [x] Update root `package.json` with build scripts
- [x] Configure TypeScript compilation for SDK and services
- [x] Set up Rust build via Cargo
- [x] Configure main entry point compilation
- [x] Test build system: `npm run build:all`

**Expected Result**: All components can be built together and dependencies work correctly

---

## Phase 2: Rust Core Development

### Task 2.1: Set up Rust Project Structure

**Current State**: Rust package exists but is basic
**Goal**: Create the Rust core project structure

**Actions**:

- [ ] Create `core/src/error.rs` with error types
- [ ] Create `core/src/models.rs` with data structures
- [ ] Create `core/src/database.rs` for database operations
- [ ] Create `core/src/state.rs` for state management
- [ ] Create `core/src/bridge.rs` for N-API bindings
- [ ] Update `core/src/lib.rs` to export modules

**Expected Result**: Rust project has proper module structure

---

### Task 2.2: Design Database Schema

**Current State**: Rust project structure exists
**Goal**: Design and implement the SQLite database schema

**Actions**:

- [x] Create `core/src/schema.sql` with table definitions
- [x] Define `workflows` table (id, name, definition, created_at)
- [x] Define `workflow_runs` table (id, workflow_id, status, payload, created_at)
- [x] Define `steps` table (id, run_id, name, status, result, created_at)
- [x] Create `core/src/database.rs` with connection setup
- [x] Add database initialization function

**Expected Result**: Database schema is defined and can be initialized

---

### Task 2.3: Create Core Data Models

**Current State**: Database schema exists
**Goal**: Define Rust structs for workflow data

**Actions**:

- [x] Define `WorkflowDefinition` struct in `core/src/models.rs`
- [x] Define `WorkflowRun` struct with status enum
- [x] Define `Step` and `StepResult` structs
- [x] Add Serde derive macros for JSON serialization
- [x] Create `WorkflowState` enum (Pending, Running, Completed, Failed)
- [x] Add validation methods to structs

**Expected Result**: Rust can serialize/deserialize workflow data

---

### Task 2.4: Implement Basic State Manager

**Current State**: Data models exist
**Goal**: Create the core state management functionality

**Actions**:

- [x] Create `StateManager` struct in `core/src/state.rs`
- [x] Implement `register_workflow` method
- [x] Implement `get_workflow` method
- [x] Implement `create_run` method
- [x] Add basic error handling with custom error types
- [x] Connect to database in StateManager

**Expected Result**: Workflows can be registered and retrieved from database

---

### Task 2.5: Create N-API Bridge

**Current State**: State manager exists
**Goal**: Create the N-API bridge for Node.js communication

**Actions**:

- [x] Set up N-API module in `core/src/bridge.rs`
- [x] Implement `register_workflow` N-API function
- [x] Add JSON serialization/deserialization
- [x] Add error handling for invalid JSON
- [x] Create basic logging for registration events
- [x] Test N-API function from Node.js

**Expected Result**: Node.js can call Rust functions via N-API

---

## Phase 3: Job Execution System

### Task 3.1: Create Job Structure

**Current State**: N-API bridge works
**Goal**: Define the job execution system

**Actions**:

- [x] Create `core/src/job.rs` with Job struct
- [x] Define job states (Pending, Running, Completed, Failed)
- [x] Add job metadata (workflowId, stepName, payload)
- [x] Create job queue structure
- [x] Add job ID generation
- [x] Add job validation methods

**Expected Result**: Job structure is defined and can be created

---

### Task 3.2: Implement Job Dispatcher

**Current State**: Job structure exists
**Goal**: Create the job dispatching system

**Actions**:

- [x] Create `core/src/dispatcher.rs` with Dispatcher struct
- [x] Implement job queue management
- [x] Add job status tracking
- [x] Create job dispatching logic
- [x] Add basic worker pool structure
- [x] Add job timeout handling

**Expected Result**: Jobs can be created and queued for execution

---

### Task 3.3: Create Context Object

**Current State**: Job dispatcher exists
**Goal**: Define the context object passed to Node.js

**Actions**:

- [ ] Create `core/src/context.rs` with Context struct
- [ ] Define context fields (payload, steps, services, run)
- [ ] Implement context serialization to JSON
- [ ] Add context creation from job data
- [ ] Create context validation
- [ ] Add context metadata (runId, workflowId, etc.)

**Expected Result**: Context objects can be created and serialized

---

### Task 3.4: Connect Job System to N-API

**Current State**: Context objects exist
**Goal**: Connect job execution to Node.js

**Actions**:

- [ ] Add `execute_job` function to `core/src/bridge.rs`
- [ ] Implement job dispatching from N-API
- [ ] Add context serialization for Node.js
- [ ] Create job result handling
- [ ] Add error handling for job execution
- [ ] Test job execution end-to-end

**Expected Result**: Rust can dispatch jobs to Node.js and handle results

---

## Phase 4: Node.js SDK Development

### Task 4.1: Create Basic Workflow Builder

**Current State**: Job execution system works
**Goal**: Create the workflow definition API

**Actions**:

- [ ] Create `sdk/src/workflow.ts` with WorkflowInstance class
- [ ] Implement `cronflow.define()` function
- [ ] Add basic workflow options (id, name, description)
- [ ] Create workflow validation
- [ ] Add workflow serialization to JSON
- [ ] Export from `sdk/src/index.ts`

**Expected Result**: Basic workflow definition works

---

### Task 4.2: Implement Step Methods

**Current State**: Basic workflow builder exists
**Goal**: Add step definition methods

**Actions**:

- [ ] Add `.step()` method to WorkflowInstance
- [ ] Add `.action()` method for side-effects
- [ ] Implement step validation
- [ ] Add step options (timeout, retry)
- [ ] Create step metadata tracking
- [ ] Add step serialization

**Expected Result**: Workflows can define steps with options

---

### Task 4.3: Add Control Flow Methods

**Current State**: Step methods exist
**Goal**: Add conditional and parallel execution

**Actions**:

- [ ] Implement `.if()/.elseIf()/.else()/.endIf()`
- [ ] Add `.parallel()` method
- [ ] Create control flow validation
- [ ] Add step dependency tracking
- [ ] Implement workflow graph building
- [ ] Add control flow serialization

**Expected Result**: Complex workflows with conditions and parallel execution

---

### Task 4.4: Connect SDK to Rust Engine

**Current State**: Workflow definition API exists
**Goal**: Connect workflow definitions to Rust engine

**Actions**:

- [ ] Add Rust addon dependency to SDK package
- [ ] Update `sdk/src/index.ts` to use Rust addon
- [ ] Add workflow registration to Rust engine
- [ ] Implement workflow serialization
- [ ] Add error handling for registration
- [ ] Test workflow registration end-to-end

**Expected Result**: Workflows defined in Node.js are registered in Rust

---

## Phase 5: Trigger System

### Task 5.1: Add Webhook Trigger

**Current State**: Workflow registration works
**Goal**: Add webhook trigger capability

**Actions**:

- [ ] Create `core/src/triggers.rs` with trigger types
- [ ] Add webhook trigger structure
- [ ] Create HTTP server setup with actix-web
- [ ] Add webhook endpoint routing
- [ ] Implement webhook payload processing
- [ ] Add webhook validation

**Expected Result**: Webhooks can trigger workflows

---

### Task 5.2: Add Schedule Trigger

**Current State**: Webhook triggers exist
**Goal**: Add scheduled trigger capability

**Actions**:

- [ ] Add cron parser dependency to core package
- [ ] Implement schedule trigger structure
- [ ] Create scheduler with time tracking
- [ ] Add schedule persistence
- [ ] Implement schedule execution
- [ ] Add timezone handling

**Expected Result**: Scheduled workflows run at correct times

---

### Task 5.3: Connect Triggers to Workflows

**Current State**: Trigger types exist
**Goal**: Connect triggers to workflow execution

**Actions**:

- [ ] Update workflow registration to include triggers
- [ ] Add trigger validation
- [ ] Implement trigger-to-workflow mapping
- [ ] Add trigger state persistence
- [ ] Test trigger execution
- [ ] Add trigger serialization

**Expected Result**: Triggers can start workflow execution

---

## Phase 6: Service Integration

### Task 6.1: Create Service Definition API

**Current State**: Triggers work
**Goal**: Add service integration framework

**Actions**:

- [ ] Create `services/src/index.ts` with service types
- [ ] Implement `defineService()` function
- [ ] Add service validation
- [ ] Create service template structure
- [ ] Add Zod schema integration
- [ ] Export service definition API

**Expected Result**: Services can be defined with validation

---

### Task 6.2: Add Service Configuration

**Current State**: Service definition API exists
**Goal**: Add service configuration and instantiation

**Actions**:

- [ ] Implement `.withConfig()` method
- [ ] Add configuration validation
- [ ] Create service instance management
- [ ] Add service dependency injection
- [ ] Test service configuration
- [ ] Add service serialization

**Expected Result**: Services can be configured and used

---

### Task 6.3: Integrate Services with SDK

**Current State**: Service configuration works
**Goal**: Make services available in workflow execution

**Actions**:

- [ ] Add services package as dependency to SDK
- [ ] Update context object to include services
- [ ] Add service method calling
- [ ] Implement service error handling
- [ ] Add service logging
- [ ] Test service integration

**Expected Result**: Services are available in `ctx.services` during execution

---

## Phase 7: Advanced Features

### Task 7.1: Add Advanced Control Flow

**Current State**: Services work
**Goal**: Add advanced workflow features

**Actions**:

- [ ] Implement `.forEach()` method
- [ ] Add `.batch()` method
- [ ] Create `.subflow()` method
- [ ] Add `.cancel()` and `.sleep()`
- [ ] Test advanced features
- [ ] Add advanced flow serialization

**Expected Result**: Advanced workflow features work correctly

---

### Task 7.2: Add Human-in-the-Loop

**Current State**: Advanced control flow exists
**Goal**: Add human approval capabilities

**Actions**:

- [ ] Implement `.humanInTheLoop()` method
- [ ] Create pause/resume token system
- [ ] Add timeout handling
- [ ] Create approval notification system
- [ ] Test human approval workflows
- [ ] Add human approval serialization

**Expected Result**: Workflows can pause for human approval

---

### Task 7.3: Add State Management

**Current State**: Human-in-the-loop works
**Goal**: Add persistent state across workflow runs

**Actions**:

- [ ] Add `ctx.state` to context object
- [ ] Implement state operations (get, set, incr)
- [ ] Add TTL support for state values
- [ ] Create state cleanup
- [ ] Test state persistence
- [ ] Add state serialization

**Expected Result**: Workflows can maintain persistent state

---

## Phase 8: Testing Framework

### Task 8.1: Create Basic Testing Harness

**Current State**: All core features work
**Goal**: Add testing capabilities

**Actions**:

- [ ] Implement `workflow.test()` method in SDK
- [ ] Create test runner with in-memory execution
- [ ] Add step mocking capabilities
- [ ] Implement basic test assertions
- [ ] Test the testing framework
- [ ] Add test utilities

**Expected Result**: Workflows can be tested in isolation

---

### Task 8.2: Add Advanced Testing Features

**Current State**: Basic testing works
**Goal**: Add comprehensive testing features

**Actions**:

- [ ] Add service mocking
- [ ] Implement trigger mocking
- [ ] Create test data generators
- [ ] Add test coverage reporting
- [ ] Test advanced testing features
- [ ] Add integration test utilities

**Expected Result**: Comprehensive testing capabilities

---

## Phase 9: Error Handling & Reliability

### Task 9.1: Implement Retry Logic

**Current State**: Testing framework works
**Goal**: Add reliability features

**Actions**:

- [ ] Implement exponential backoff in core
- [ ] Add fixed delay retry strategy
- [ ] Create retry attempt tracking
- [ ] Add retry exhaustion handling
- [ ] Test retry logic
- [ ] Add retry configuration

**Expected Result**: Failed steps are retried automatically

---

### Task 9.2: Add Circuit Breaker

**Current State**: Retry logic works
**Goal**: Add circuit breaker pattern

**Actions**:

- [ ] Implement circuit breaker pattern in core
- [ ] Add failure threshold detection
- [ ] Create circuit state management
- [ ] Add circuit recovery logic
- [ ] Test circuit breaker
- [ ] Add circuit breaker configuration

**Expected Result**: Cascading failures are prevented

---

### Task 9.3: Add Monitoring

**Current State**: Circuit breaker works
**Goal**: Add observability features

**Actions**:

- [ ] Add structured logging to core
- [ ] Implement metrics collection
- [ ] Create health check endpoints
- [ ] Add performance monitoring
- [ ] Test monitoring features
- [ ] Add monitoring configuration

**Expected Result**: System is observable and monitorable

---

## Phase 10: Production Readiness

### Task 10.1: Performance Optimization

**Current State**: All features work
**Goal**: Optimize for production

**Actions**:

- [ ] Optimize database queries in core
- [ ] Implement connection pooling
- [ ] Add caching for frequently accessed data
- [ ] Optimize JSON serialization
- [ ] Test performance improvements
- [ ] Add performance monitoring

**Expected Result**: System performs well under load

---

### Task 10.2: Add Production Features

**Current State**: Performance is optimized
**Goal**: Add production-ready features

**Actions**:

- [ ] Add graceful shutdown handling
- [ ] Implement configuration management
- [ ] Create backup and recovery systems
- [ ] Add security features
- [ ] Test production features
- [ ] Add production configuration

**Expected Result**: System is production-ready

---

### Task 10.3: Complete Documentation

**Current State**: Production features work
**Goal**: Complete documentation and examples

**Actions**:

- [ ] Complete API documentation
- [ ] Create comprehensive examples
- [ ] Add deployment guides
- [ ] Create troubleshooting guides
- [ ] Test documentation
- [ ] Add getting started guide

**Expected Result**: System is well-documented and easy to use

---

## Monolith Architecture Benefits

### Development Workflow:

- **Build**: `npm run build:all` builds all components
- **Test**: `npm test` tests all components
- **Dev**: `npm run dev` runs dev mode
- **Publish**: `npm run release` creates new version

### Package Dependencies:

- SDK depends on Core (Rust addon)
- Services depends on SDK
- Main entry point exports everything
- Single npm package for users

### Tooling:

- **TypeScript**: Shared configuration with path mapping
- **Rust**: Cargo for core engine compilation
- **npm**: Single package management
- **Semantic Release**: Automatic versioning

### Configuration:

- Shared TypeScript configuration
- Centralized build scripts
- Single package.json for dependencies
- Optimized build and test pipelines

This monolith structure provides the best development experience for a Node.js + Rust project, with clear separation of concerns, efficient tooling, and excellent simplicity for both development and deployment.
