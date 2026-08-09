name: etl_job
parameters:
  - name: env
    default: prod
tasks:
  - task_key: ingest
    notebook_task:
      notebook_path: /notebooks/ingest
  - task_key: sanity_probe
    notebook_task:
      notebook_path: /notebooks/sanity_probe
  - task_key: audit_snapshot
    notebook_task:
      notebook_path: /notebooks/audit_snapshot
  - task_key: transform
    depends_on:
      - task_key: ingest
    notebook_task:
      notebook_path: /notebooks/transform
