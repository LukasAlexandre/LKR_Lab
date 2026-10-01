# Dependencies and licenses

Versions resolved from npm and crates.io on 2026-09-19. Direct dependencies reviewed for declared license; not a substitute for a full distribution/legal audit. No license was selected for proprietary project code.

## JavaScript direct dependencies

| Package | Version | License |
|---|---|---|
| @eslint/js | 10.0.1 | MIT |
| @tauri-apps/api | 2.11.1 | Apache-2.0 OR MIT |
| @tauri-apps/cli | 2.11.4 | Apache-2.0 OR MIT |
| @types/react | 19.3.0 | MIT |
| @types/react-dom | 19.3.0 | MIT |
| @vitejs/plugin-react | 6.1.1 | MIT |
| eslint | 10.11.0 | MIT |
| lucide-react | 1.47.0 | ISC |
| react | 19.3.0 | MIT |
| react-dom | 19.3.0 | MIT |
| typescript | 6.0.3 | Apache-2.0 |
| typescript-eslint | 8.70.0 | MIT |
| vite | 8.3.0 | MIT |
| vitest | 4.1.11 | MIT |

## Rust direct dependencies

| Crate | Version | License |
|---|---|---|
| tauri | 2.11.5 | Apache-2.0 OR MIT |
| tauri-build | 2.6.3 | Apache-2.0 OR MIT |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 |
| rusqlite | 0.40.2 | MIT |
| uuid | 1.26.1 | Apache-2.0 OR MIT |
| sysinfo | 0.39.6 | MIT |
| netstat2 | 0.11.2 | MIT OR Apache-2.0 |
| wait-timeout | 0.2.1 | MIT/Apache-2.0 |
| tempfile | 3.27.0 | MIT OR Apache-2.0 |
| rfd | 0.17.2 | MIT |

SQLite: public domain upstream. Lucide: ISC. React/Vite/ESLint tooling: licenses above. Tauri: MIT OR Apache-2.0. Cargo.lock and package-lock.json pin the complete dependency trees.

Before distributing installers, collect complete transitive notices and re-check advisory feeds. The application does not ship its repository or external CLI binaries.

Sources: [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), [Vite guide](https://vite.dev/guide/), [React versions](https://react.dev/versions), npm package manifests and crates.io package manifests.
