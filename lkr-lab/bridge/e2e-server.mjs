// Bridge de teste: sobe o MESMO servidor do LKR LAB sobre um repositório temporário,
// numa porta livre, para os testes de integração do desktop (crates/hub-core/tests).
// Uso: node lkr-lab/bridge/e2e-server.mjs <repoRoot>   → imprime "PORT <n>".
import { createGitBackup } from "./git-backup.mjs";
import { createLabServer } from "./server.mjs";

const repoRoot = process.argv[2];
if (!repoRoot) {
  console.error("uso: e2e-server.mjs <repoRoot>");
  process.exit(2);
}
const server = createLabServer({ backup: createGitBackup({ repoRoot }) });
server.listen(0, "127.0.0.1", () => console.log("PORT " + server.address().port));
