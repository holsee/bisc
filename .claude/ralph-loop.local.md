---
active: true
iteration: 1
max_iterations: 50
completion_promise: "ALL_TASKS_COMPLETE"
started_at: "2026-02-15T00:58:38Z"
---

Read CLAUDE.md for your full workflow. Pick the next eligible task from tasks/TODO/ (lowest number whose dependencies are all in tasks/DONE/), implement it, validate with cargo build/fmt/clippy/test, review test.log for warnings/errors, verify all acceptance criteria, move the task file to tasks/DONE/, commit as BISC-XXX, record any learnings, then pick the next task. When all tasks in tasks/TODO/ are complete, output: <promise>ALL_TASKS_COMPLETE</promise>
