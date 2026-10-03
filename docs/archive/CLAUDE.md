# Continuum

## Task Observer activation

Before the first tool call of any session — and before writing or proposing a plan, not merely before executing one — invoke the task-observer skill AND execute its Session Start Protocol (storage check, frontmatter scan, review trigger). Loading the skill and running the protocol are separate steps; a session that loads the file and stops has activated nothing. Any turn that will involve a tool call counts; do not classify the session as "too simple" from its opening message.

After completing each task, check the observation records written this session and report a one-line summary (ids and titles, or "none logged and why").

The task-observer workspace for this project is:
  /workspace/Continuum
Every path the skill uses derives from that root and nothing else:
  /workspace/Continuum/skill-observations/observation-log/
  /workspace/Continuum/skill-observations/cross-cutting-principles.md
  /workspace/Continuum/skill-updates/
  /workspace/Continuum/skill-updates/PENDING.md
Never resolve any of them from the current working directory.
