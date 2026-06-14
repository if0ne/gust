CREATE TABLE IF NOT EXISTS workflow (
    workflow_id      TEXT PRIMARY KEY,
    yaml_source TEXT        NOT NULL,
    spec        JSONB       NOT NULL,
    schedule    TEXT        NOT NULL,
    catchup     BOOLEAN     NOT NULL DEFAULT false,
    is_active   BOOLEAN     NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS workflow_run (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    workflow_id       TEXT        NOT NULL REFERENCES workflow(workflow_id),
    logical_date TIMESTAMPTZ NOT NULL,
    state        TEXT        NOT NULL DEFAULT 'queued',
    run_type     TEXT        NOT NULL DEFAULT 'scheduled',
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS workflow_run_workflow_id_logical_date ON workflow_run(workflow_id, logical_date);
CREATE INDEX IF NOT EXISTS workflow_run_workflow_id_state ON workflow_run(workflow_id, state);

CREATE TABLE IF NOT EXISTS task_instance (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    workflow_run_id   UUID        NOT NULL REFERENCES workflow_run(id),
    task_id      TEXT        NOT NULL,
    state        TEXT        NOT NULL DEFAULT 'pending',
    try_number   INT         NOT NULL DEFAULT 0,
    max_retries  INT         NOT NULL DEFAULT 0,
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ,
    exit_code    INT,
    error        TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS task_instance_run_task ON task_instance(workflow_run_id, task_id);
CREATE INDEX IF NOT EXISTS task_instance_state ON task_instance(state);

CREATE TABLE IF NOT EXISTS task_log (
    id                BIGSERIAL   PRIMARY KEY,
    task_instance_id  UUID        NOT NULL REFERENCES task_instance(id),
    try_number        INT         NOT NULL,
    stream            TEXT        NOT NULL,
    content           TEXT        NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS task_log_instance ON task_log(task_instance_id);
