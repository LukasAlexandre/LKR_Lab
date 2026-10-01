/*
 * LKR LAB — bridge local
 *
 *   npm run lab   →   http://127.0.0.1:4317/lab-setup/
 *
 * Serve os arquivos do LKR LAB e uma API mínima de backup no Git.
 * Escuta somente em 127.0.0.1. Não existe endpoint de comando genérico:
 * cada rota chama uma operação fixa de git-backup.mjs.
 *
 * Proteções para uso local:
 *  - Host precisa ser 127.0.0.1/localhost na porta do bridge (bloqueia DNS rebinding);
 *  - toda rota /api exige o cabeçalho X-LKR-Lab, que outra origem não consegue
 *    enviar sem preflight CORS (e o bridge não responde a CORS);
 *  - POST exige Content-Type application/json e Origin igual à do bridge.
 *
 * Clientes: as páginas do LKR LAB (mesma origem) e o app desktop, cujo backend
 * Rust chama só as rotas fixas de estado/sync do módulo "workspace".
 */
import http from "node:http";
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { BackupError, createGitBackup } from "./git-backup.mjs";

const HOST = "127.0.0.1";
const DEFAULT_PORT = 4317;
const MAX_BODY = 1024 * 1024;
const LAB_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const REPO_ROOT = path.resolve(LAB_ROOT, "..");
// Só estas pastas do LKR LAB são servidas como arquivos estáticos.
const STATIC_DIRS = new Set(["core", "lab-setup"]);
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
};

function send(res, status, body, headers = {}) {
  res.writeHead(status, {
    "X-Content-Type-Options": "nosniff",
    "Referrer-Policy": "no-referrer",
    "Cross-Origin-Resource-Policy": "same-origin",
    "Content-Length": Buffer.byteLength(body),
    ...headers,
  });
  res.end(body);
}

function sendJson(res, status, data) {
  send(res, status, JSON.stringify({ bridge: "lkr-lab", ...data }), { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" });
}

function sendError(res, error) {
  if (error instanceof BackupError) {
    return sendJson(res, error.status, { success: false, error: { code: error.code, message: error.message, detail: error.detail || undefined, commit: error.commit } });
  }
  console.error("[lkr-lab] erro inesperado:", error);
  return sendJson(res, 500, { success: false, error: { code: "INTERNAL", message: "Erro interno do bridge." } });
}

function readJsonBody(req) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on("data", (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY) {
        reject(new BackupError("INVALID_PAYLOAD", { status: 413, message: "Payload grande demais." }));
        req.destroy();
      } else chunks.push(chunk);
    });
    req.on("end", () => {
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      } catch {
        reject(new BackupError("INVALID_PAYLOAD", { status: 400, message: "JSON inválido." }));
      }
    });
    req.on("error", reject);
  });
}

async function serveStatic(req, res, pathname) {
  if (pathname === "/" || pathname === "/lab-setup") return send(res, 302, "", { Location: "/lab-setup/" });
  let relative;
  try {
    relative = decodeURIComponent(pathname).replace(/^\/+/, "");
  } catch {
    return send(res, 400, "Bad request");
  }
  if (relative.endsWith("/") || relative === "") relative += "index.html";
  const target = path.resolve(LAB_ROOT, relative);
  const inside = path.relative(LAB_ROOT, target);
  const topDir = inside.split(path.sep)[0];
  if (inside.startsWith("..") || path.isAbsolute(inside) || !STATIC_DIRS.has(topDir) || /\.test\./.test(target)) {
    return send(res, 404, "Not found", { "Content-Type": "text/plain; charset=utf-8" });
  }
  try {
    const body = await fs.readFile(target);
    const type = TYPES[path.extname(target).toLowerCase()] || "application/octet-stream";
    send(res, 200, req.method === "HEAD" ? "" : body, { "Content-Type": type, "Cache-Control": "no-cache" });
  } catch {
    send(res, 404, "Not found", { "Content-Type": "text/plain; charset=utf-8" });
  }
}

/**
 * @param {object} [options]
 * @param {ReturnType<typeof createGitBackup>} [options.backup]
 */
export function createLabServer({ backup = createGitBackup({ repoRoot: REPO_ROOT }) } = {}) {
  const server = http.createServer(async (req, res) => {
    const port = server.address() && server.address().port;
    const allowedHosts = new Set([HOST + ":" + port, "localhost:" + port]);
    const allowedOrigins = new Set([...allowedHosts].map((h) => "http://" + h));

    if (!allowedHosts.has(String(req.headers.host || "").toLowerCase())) return send(res, 421, "Misdirected request");

    const url = new URL(req.url, "http://" + req.headers.host);
    if (!url.pathname.startsWith("/api/")) {
      if (req.method !== "GET" && req.method !== "HEAD") return send(res, 405, "Method not allowed", { Allow: "GET, HEAD" });
      // Uma origem só: o cache local do navegador é separado por endereço.
      if (url.hostname === "localhost") return send(res, 308, "", { Location: "http://" + HOST + ":" + port + url.pathname + url.search });
      return serveStatic(req, res, url.pathname);
    }

    if (req.headers["x-lkr-lab"] !== "1") return sendJson(res, 403, { success: false, error: { code: "FORBIDDEN", message: "Requisição recusada." } });
    const fetchSite = req.headers["sec-fetch-site"];
    if (fetchSite && fetchSite !== "same-origin" && fetchSite !== "none") {
      return sendJson(res, 403, { success: false, error: { code: "FORBIDDEN", message: "Origem não permitida." } });
    }
    if (req.method === "POST") {
      if (!allowedOrigins.has(String(req.headers.origin || ""))) return sendJson(res, 403, { success: false, error: { code: "FORBIDDEN", message: "Origem não permitida." } });
      if (!/^application\/json\b/i.test(String(req.headers["content-type"] || ""))) {
        return sendJson(res, 415, { success: false, error: { code: "UNSUPPORTED", message: "Use application/json." } });
      }
    }

    // Rotas por módulo (lab-setup, workspace…): o id só escolhe uma entrada fixa de MODULES.
    const moduleRoute = /^\/api\/(state|sync|update|remote)\/([a-z][a-z-]{0,39})$/.exec(url.pathname);
    const route = moduleRoute ? req.method + " /api/" + moduleRoute[1] + "/:module" : req.method + " " + url.pathname;
    const moduleId = moduleRoute ? moduleRoute[2] : url.searchParams.get("module") || "lab-setup";
    try {
      switch (route) {
        case "GET /api/git/status":
          return sendJson(res, 200, await backup.status({ fetch: url.searchParams.get("fetch") === "1", module: moduleId }));
        case "GET /api/state/:module":
          return sendJson(res, 200, { success: true, ...(await backup.readState(moduleId)) });
        case "POST /api/sync/:module":
          return sendJson(res, 200, await backup.sync(moduleId, await readJsonBody(req)));
        case "GET /api/remote/:module":
          return sendJson(res, 200, { success: true, ...(await backup.remoteState(moduleId, { fetch: url.searchParams.get("fetch") !== "0" })) });
        case "POST /api/update/:module":
          return sendJson(res, 200, await backup.update(moduleId, { strict: url.searchParams.get("strict") === "1" }));
        case "GET /api/lab-state":
          return sendJson(res, 200, { success: true, ...(await backup.readState("lab-setup")) });
        case "POST /api/lab-sync":
          return sendJson(res, 200, await backup.sync("lab-setup", await readJsonBody(req)));
        case "POST /api/lab-update":
          return sendJson(res, 200, await backup.update("lab-setup"));
        default:
          return sendJson(res, 404, { success: false, error: { code: "NOT_FOUND", message: "Rota inexistente." } });
      }
    } catch (error) {
      return sendError(res, error);
    }
  });
  return server;
}

// Execução direta: `node lkr-lab/bridge/server.mjs`
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const port = Number(process.env.LKR_LAB_PORT) || DEFAULT_PORT;
  const server = createLabServer();
  server.on("error", (error) => {
    console.error(error.code === "EADDRINUSE" ? `[lkr-lab] A porta ${port} já está em uso. Defina LKR_LAB_PORT para usar outra.` : error);
    process.exit(1);
  });
  server.listen(port, HOST, async () => {
    const status = await createGitBackup({ repoRoot: REPO_ROOT }).status().catch(() => null);
    console.log(`\n  LKR LAB bridge  →  http://${HOST}:${port}/lab-setup/`);
    if (status && status.repository) console.log(`  Repositório ${status.repo || "(sem origin)"} · branch ${status.branch} · dados em ${status.backup.file}`);
    else console.log("  Aviso: repositório Git não encontrado; o backup no GitHub ficará indisponível.");
    console.log("  Somente 127.0.0.1. Ctrl+C para encerrar.\n");
  });
}
