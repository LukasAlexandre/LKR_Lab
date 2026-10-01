# Performance contract

LKR LAB is a local-first desktop tool. UI navigation must never wait for
OS, Git, GitHub, or agent-provider collection.

## Budgets

| Interaction | Budget |
|---|---:|
| Hover / pressed feedback | 50 ms |
| Command Center open | 100 ms |
| Local fuzzy-search update | 50 ms |
| Cached route switch | 100 ms perceived |
| Popover / dropdown motion | 180 ms |
| Page transition motion | 220 ms |
| Main-thread long task | < 50 ms |

Background operations have no fake minimum duration. Existing data remains
visible while a source revalidates. A failure in one source must not erase data
from another source.

## Current baseline

- Production renderer: 286.77 kB JS / 88.07 kB gzip.
- CSS: 25.10 kB / 6.27 kB gzip.
- Local fuzzy search has no network or IPC dependency.
- Git and agent context keep a per-project in-memory stale value during refresh.
- CLI presence is detected once per application process.
- CPU inspection no longer sleeps inside a backend operation.

## Known hot paths

- Ports and processes currently originate from separate IPC commands and may
  enumerate the process table twice during a full refresh.
- `App.tsx` still owns broad workspace orchestration; further feature extraction
  should isolate route-level rerenders.
- Very large process/socket collections are capped in rendering, but true row
  virtualization is still pending.
- Git state is queried per active project and is not persisted across restarts.

Measure command duration at the Tauri boundary before adding polling. Prefer
source-specific refresh timestamps and opt-in intervals over a single global
timer.
