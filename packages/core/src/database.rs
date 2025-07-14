//! Database operations for the Node-Cronflow Core Engine

use rusqlite::Connection;
use crate::error::CoreResult;
use crate::models::{WorkflowDefinition, WorkflowRun, StepResult};

/// Database connection wrapper
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Create a new database connection
    pub fn new(path: &str) -> CoreResult<Self> {
        let conn = Connection::open(path)?;
        let db = Database { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Initialize database schema
    fn init_schema(&self) -> CoreResult<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS workflows (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                definition TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS workflow_runs (
                id TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                status TEXT NOT NULL,
                payload TEXT NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                error TEXT,
                FOREIGN KEY (workflow_id) REFERENCES workflows (id)
            );

            CREATE TABLE IF NOT EXISTS step_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                status TEXT NOT NULL,
                output TEXT,
                error TEXT,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                duration_ms INTEGER,
                FOREIGN KEY (run_id) REFERENCES workflow_runs (id)
            );

            CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow_id ON workflow_runs (workflow_id);
            CREATE INDEX IF NOT EXISTS idx_step_results_run_id ON step_results (run_id);
            "#,
        )?;
        Ok(())
    }

    /// Save a workflow definition
    pub fn save_workflow(&self, workflow: &WorkflowDefinition) -> CoreResult<()> {
        let definition = serde_json::to_string(workflow)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO workflows (id, name, description, definition, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            (
                &workflow.id,
                &workflow.name,
                &workflow.description,
                &definition,
                &workflow.created_at.to_rfc3339(),
                &workflow.updated_at.to_rfc3339(),
            ),
        )?;
        Ok(())
    }

    /// Get a workflow definition by ID
    pub fn get_workflow(&self, id: &str) -> CoreResult<Option<WorkflowDefinition>> {
        let mut stmt = self.conn.prepare(
            "SELECT definition FROM workflows WHERE id = ?"
        )?;
        
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            let definition: String = row.get(0)?;
            let workflow: WorkflowDefinition = serde_json::from_str(&definition)?;
            Ok(Some(workflow))
        } else {
            Ok(None)
        }
    }

    /// Save a workflow run
    pub fn save_run(&self, run: &WorkflowRun) -> CoreResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO workflow_runs (id, workflow_id, status, payload, started_at, completed_at, error) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                &run.id.to_string(),
                &run.workflow_id,
                &format!("{:?}", run.status),
                &serde_json::to_string(&run.payload)?,
                &run.started_at.to_rfc3339(),
                &run.completed_at.map(|dt| dt.to_rfc3339()),
                &run.error,
            ),
        )?;
        Ok(())
    }

    /// Save a step result
    pub fn save_step_result(&self, result: &StepResult, run_id: &str) -> CoreResult<()> {
        self.conn.execute(
            "INSERT INTO step_results (run_id, step_id, status, output, error, started_at, completed_at, duration_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (
                run_id,
                &result.step_id,
                &format!("{:?}", result.status),
                &result.output.as_ref().map(|v| serde_json::to_string(v)).transpose()?,
                &result.error,
                &result.started_at.to_rfc3339(),
                &result.completed_at.map(|dt| dt.to_rfc3339()),
                &result.duration_ms,
            ),
        )?;
        Ok(())
    }
} 