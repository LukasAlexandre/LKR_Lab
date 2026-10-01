//! Cliente do bridge local do LKR LAB (lkr-lab/bridge/server.mjs).
//!
//! O Git roda SOMENTE no bridge (git-backup.mjs): ele isola o commit ao arquivo
//! do módulo, recusa merge/rebase/detached HEAD e nunca força push. Este cliente
//! só conversa HTTP com 127.0.0.1 — sem TLS, sem redirecionamento, sem URL vinda
//! de dados — e confere que quem respondeu é mesmo o bridge.
use crate::portable::PortableWorkspace;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

pub const DEFAULT_PORT: u16 = 4317;
const MODULE: &str = "workspace";
const MAX_RESPONSE: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeError {
    /// Bridge fora do ar ou resposta que não é dele: o app segue local.
    Unavailable(String),
    /// O bridge respondeu com erro (código estável + mensagem já amigável).
    Rejected { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteFile {
    /// Branch ou arquivo ainda não existem no remote.
    Missing,
    Invalid(String),
    /// Criado por versão futura do LKR LAB: nunca aplicado.
    Newer(String),
    Ok(Box<PortableWorkspace>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteInfo {
    pub has_remote: bool,
    pub fetched: bool,
    pub fetch_error: Option<(String, String)>,
    pub file: RemoteFile,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PushOutcome {
    pub pushed: bool,
    pub commit: Option<String>,
}

/// Fronteira usada pelo sync; os testes de lógica usam uma implementação falsa.
pub trait Bridge {
    /// Fetch + arquivo no ref remoto (a árvore de trabalho não é tocada).
    fn remote(&self) -> Result<RemoteInfo, BridgeError>;
    /// Grava data/workspace.json, commita SÓ esse arquivo e publica.
    fn push(&self, ws: &PortableWorkspace) -> Result<PushOutcome, BridgeError>;
    /// Fast-forward do repositório; recusa se houver alterações de desenvolvimento.
    fn update(&self) -> Result<(), BridgeError>;
}

pub struct HttpBridge {
    addr: SocketAddr,
}

impl HttpBridge {
    pub fn new(port: u16) -> Self {
        Self {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
        }
    }
    /// Porta de LKR_LAB_PORT (a mesma do `npm run lab`) ou a padrão.
    pub fn from_env() -> Self {
        let port = std::env::var("LKR_LAB_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|p| *p != 0)
            .unwrap_or(DEFAULT_PORT);
        Self::new(port)
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        read_timeout: Duration,
    ) -> Result<Value, BridgeError> {
        let down = |e: &dyn std::fmt::Display| BridgeError::Unavailable(e.to_string());
        let mut stream = TcpStream::connect_timeout(&self.addr, Duration::from_millis(800))
            .map_err(|e| down(&e))?;
        stream
            .set_read_timeout(Some(read_timeout))
            .map_err(|e| down(&e))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| down(&e))?;
        let origin = format!("127.0.0.1:{}", self.addr.port());
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {origin}\r\nX-LKR-Lab: 1\r\nAccept: application/json\r\nConnection: close\r\n"
        );
        if let Some(body) = body {
            head.push_str(&format!(
                "Origin: http://{origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).map_err(|e| down(&e))?;
        if let Some(body) = body {
            stream.write_all(body.as_bytes()).map_err(|e| down(&e))?;
        }
        let mut raw = Vec::new();
        stream
            .take(MAX_RESPONSE)
            .read_to_end(&mut raw)
            .map_err(|e| down(&e))?;
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| BridgeError::Unavailable("resposta inválida".into()))?;
        let headers = String::from_utf8_lossy(&raw[..split]).to_lowercase();
        if headers.contains("transfer-encoding: chunked") {
            return Err(BridgeError::Unavailable("resposta inesperada".into()));
        }
        let value: Value = serde_json::from_slice(&raw[split + 4..])
            .map_err(|_| BridgeError::Unavailable("resposta inválida".into()))?;
        if value["bridge"] != "lkr-lab" {
            return Err(BridgeError::Unavailable(
                "a porta não é do bridge do LKR LAB".into(),
            ));
        }
        if value["success"] == false {
            return Err(BridgeError::Rejected {
                code: value["error"]["code"].as_str().unwrap_or("ERROR").into(),
                message: value["error"]["message"]
                    .as_str()
                    .unwrap_or("O bridge recusou a operação.")
                    .into(),
            });
        }
        Ok(value)
    }
}

impl Bridge for HttpBridge {
    fn remote(&self) -> Result<RemoteInfo, BridgeError> {
        // O fetch pode levar até 60 s no Git do bridge.
        let value = self.request(
            "GET",
            &format!("/api/remote/{MODULE}"),
            None,
            Duration::from_secs(75),
        )?;
        let file = &value["file"];
        let message = || file["message"].as_str().unwrap_or("").to_string();
        let file = match file["status"].as_str() {
            Some("ok") => {
                match serde_json::from_value::<PortableWorkspace>(file["state"].clone()) {
                    Ok(ws) => RemoteFile::Ok(Box::new(ws)),
                    Err(_) => RemoteFile::Invalid("formato inesperado".into()),
                }
            }
            Some("invalid") => RemoteFile::Invalid(message()),
            Some("newer") => RemoteFile::Newer(message()),
            _ => RemoteFile::Missing,
        };
        let fetch_error = value["fetchError"]["code"].as_str().map(|code| {
            (
                code.to_string(),
                value["fetchError"]["message"].as_str().unwrap_or("").into(),
            )
        });
        Ok(RemoteInfo {
            has_remote: value["remote"].as_bool().unwrap_or(false),
            fetched: value["fetched"].as_bool().unwrap_or(false),
            fetch_error,
            file,
        })
    }
    fn push(&self, ws: &PortableWorkspace) -> Result<PushOutcome, BridgeError> {
        let body = json!({ "schemaVersion": crate::portable::SCHEMA_VERSION, "state": ws });
        let value = self.request(
            "POST",
            &format!("/api/sync/{MODULE}"),
            Some(&body.to_string()),
            Duration::from_secs(120),
        )?;
        Ok(PushOutcome {
            pushed: value["pushed"].as_bool().unwrap_or(false),
            commit: value["commit"].as_str().map(str::to_string),
        })
    }
    fn update(&self) -> Result<(), BridgeError> {
        self.request(
            "POST",
            &format!("/api/update/{MODULE}?strict=1"),
            Some("{}"),
            Duration::from_secs(120),
        )?;
        Ok(())
    }
}
