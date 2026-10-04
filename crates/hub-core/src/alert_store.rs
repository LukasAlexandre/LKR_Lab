//! Persistência LOCAL dos alertas e do histórico de diagnósticos (SESSION-002, Block 09).
//!
//! `hub.db` é por máquina; estas tabelas (migração 011) descrevem ESTA máquina e nunca entram no
//! workspace portátil, no sync, no Git, no Planejamento nem no DDAE. O ciclo de vida (o que abre,
//! reconhece, resolve e reabre) é decidido por `diagnostics::reconcile`; aqui só se grava.
use crate::{
    database::Database,
    diagnostic_runner::{DiagnosticId, RunRecord, RunResult, HISTORY_KEEP},
    diagnostics::{
        self, AlertRecord, AlertStatus, AlertsSnapshot, Change, Confidence, Cta, DiagnosticRef,
        DiagnosticsView, Domain, Evidence, Facts, Finding, Severity, RESOLVED_VISIBLE_MS,
    },
    windows_health::Millis,
    HubResult,
};
use rusqlite::{params, OptionalExtension, Row};

/// Alertas resolvidos guardados no máximo (além do prazo de `RESOLVED_VISIBLE_MS`, o mais antigo sai).
pub const RESOLVED_KEEP: i64 = 500;
/// O histórico resolvido é podado depois disto.
pub const RESOLVED_RETENTION_MS: Millis = 30 * 24 * 3_600_000;

const COLUMNS: &str = "id, fingerprint, rule_id, domain, source, resource, severity, confidence, title, summary, reason, next_step, evidence, action, cta, status, first_seen, last_seen, acknowledged_at, resolved_at, occurrence, seen_count";

fn db_err(error: rusqlite::Error) -> String {
    error.to_string()
}

fn read_alert(row: &Row) -> rusqlite::Result<AlertRecord> {
    let text = |i: usize| row.get::<_, String>(i);
    let evidence: Vec<Evidence> = serde_json::from_str(&text(12)?).unwrap_or_default();
    let action: Option<DiagnosticRef> = row
        .get::<_, Option<String>>(13)?
        .and_then(|json| serde_json::from_str(&json).ok());
    let cta: Option<Cta> = row
        .get::<_, Option<String>>(14)?
        .and_then(|json| serde_json::from_str(&json).ok());
    Ok(AlertRecord {
        id: text(0)?,
        finding: Finding {
            id: text(1)?,
            fingerprint: text(1)?,
            rule_id: text(2)?,
            domain: Domain::parse(&text(3)?).unwrap_or(Domain::Machine),
            source: text(4)?,
            resource: text(5)?,
            severity: Severity::parse(&text(6)?).unwrap_or(Severity::Info),
            confidence: Confidence::parse(&text(7)?).unwrap_or(Confidence::Low),
            title: text(8)?,
            summary: text(9)?,
            reason: text(10)?,
            recommended_next_step: text(11)?,
            evidence,
            diagnostic_action: action,
            cta,
        },
        status: AlertStatus::parse(&text(15)?).unwrap_or(AlertStatus::Active),
        first_seen: row.get(16)?,
        last_seen: row.get(17)?,
        acknowledged_at: row.get(18)?,
        resolved_at: row.get(19)?,
        occurrence_count: row.get(20)?,
        observations: row.get(21)?,
    })
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

impl Database {
    /// Avalia os fatos, aplica o ciclo de vida ao histórico local e devolve a visão completa.
    /// Cada chamada é barata: só regras puras e algumas linhas do banco.
    pub fn alerts_run(
        &self,
        facts: &Facts,
        now: Millis,
        diagnostics: DiagnosticsView,
    ) -> HubResult<AlertsSnapshot> {
        let latest = self.alerts_latest()?;
        let evaluation = diagnostics::evaluate(now, facts, &diagnostics::previous_of(&latest));
        let changes =
            diagnostics::reconcile(&latest, &evaluation, now, &mut diagnostics::new_alert_id);
        self.alerts_apply(&changes)?;
        self.alerts_prune(now)?;
        let mut alerts = self.alerts_visible(now)?;
        diagnostics::sort_alerts(&mut alerts);
        Ok(AlertsSnapshot {
            captured_at: now,
            summary: diagnostics::summarize(&alerts, now),
            alerts,
            sources: evaluation.statuses,
            diagnostics,
        })
    }

    /// A ocorrência mais recente de cada fingerprint (qualquer status).
    pub fn alerts_latest(&self) -> HubResult<Vec<AlertRecord>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM machine_alerts a WHERE occurrence = (SELECT MAX(occurrence) FROM machine_alerts b WHERE b.fingerprint = a.fingerprint)"
            ))
            .map_err(db_err)?;
        let rows = stmt.query_map([], read_alert).map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Abertos (ativos e reconhecidos) mais os resolvidos que ainda estão dentro da janela visível.
    pub fn alerts_visible(&self, now: Millis) -> HubResult<Vec<AlertRecord>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM machine_alerts WHERE status <> 'resolved' OR resolved_at >= ?1 ORDER BY first_seen DESC LIMIT 400"
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![now - RESOLVED_VISIBLE_MS], read_alert)
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Aplica o resultado de `reconcile` de forma atômica.
    pub fn alerts_apply(&self, changes: &[Change]) -> HubResult<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction().map_err(db_err)?;
        for change in changes {
            match change {
                Change::Insert(r) => {
                    tx.execute(
                        &format!("INSERT INTO machine_alerts ({COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)"),
                        params![
                            r.id,
                            r.finding.fingerprint,
                            r.finding.rule_id,
                            r.finding.domain.as_str(),
                            r.finding.source,
                            r.finding.resource,
                            r.finding.severity.as_str(),
                            r.finding.confidence.as_str(),
                            r.finding.title,
                            r.finding.summary,
                            r.finding.reason,
                            r.finding.recommended_next_step,
                            json(&r.finding.evidence),
                            r.finding.diagnostic_action.as_ref().map(json),
                            r.finding.cta.as_ref().map(json),
                            r.status.as_str(),
                            r.first_seen,
                            r.last_seen,
                            r.acknowledged_at,
                            r.resolved_at,
                            r.occurrence_count,
                            r.observations,
                        ],
                    )
                    .map_err(db_err)?;
                }
                Change::Update(r) => {
                    tx.execute(
                        "UPDATE machine_alerts SET severity=?2, confidence=?3, title=?4, summary=?5, reason=?6, next_step=?7, evidence=?8, action=?9, cta=?10, status=?11, last_seen=?12, acknowledged_at=?13, resolved_at=?14, seen_count=?15 WHERE id=?1",
                        params![
                            r.id,
                            r.finding.severity.as_str(),
                            r.finding.confidence.as_str(),
                            r.finding.title,
                            r.finding.summary,
                            r.finding.reason,
                            r.finding.recommended_next_step,
                            json(&r.finding.evidence),
                            r.finding.diagnostic_action.as_ref().map(json),
                            r.finding.cta.as_ref().map(json),
                            r.status.as_str(),
                            r.last_seen,
                            r.acknowledged_at,
                            r.resolved_at,
                            r.observations,
                        ],
                    )
                    .map_err(db_err)?;
                }
            }
        }
        tx.commit().map_err(db_err)
    }

    /// Reconhece um alerta (só o ciclo de vida local; nada muda na máquina).
    pub fn alert_acknowledge(&self, id: &str, now: Millis) -> HubResult<AlertRecord> {
        let record: Option<AlertRecord> = self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM machine_alerts WHERE id = ?1"),
                params![id],
                read_alert,
            )
            .optional()
            .map_err(db_err)?;
        let record = record.ok_or("Alerta não encontrado.")?;
        match crate::diagnostics::acknowledge(&record, now) {
            Some(next) => {
                self.alerts_apply(&[Change::Update(next.clone())])?;
                Ok(next)
            }
            None if record.status == AlertStatus::Resolved => {
                Err("O alerta já foi resolvido.".into())
            }
            None => Ok(record),
        }
    }

    /// Poda o histórico resolvido (prazo e quantidade). Alertas abertos nunca são podados.
    pub fn alerts_prune(&self, now: Millis) -> HubResult<()> {
        self.conn
            .execute(
                "DELETE FROM machine_alerts WHERE status = 'resolved' AND resolved_at < ?1",
                params![now - RESOLVED_RETENTION_MS],
            )
            .map_err(db_err)?;
        self.conn
            .execute(
                "DELETE FROM machine_alerts WHERE status = 'resolved' AND id NOT IN (SELECT id FROM machine_alerts WHERE status = 'resolved' ORDER BY resolved_at DESC LIMIT ?1)",
                params![RESOLVED_KEEP],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Grava um diagnóstico terminado (resumo e cauda curta; nunca a saída inteira).
    pub fn diagnostic_run_save(&self, run: &RunRecord) -> HubResult<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO machine_diagnostic_runs (id, diagnostic, target, started_at, finished_at, result, exit_code, summary, output_tail) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    run.id,
                    run.diagnostic,
                    run.target,
                    run.started_at,
                    run.finished_at,
                    run.result.map(RunResult::as_str).unwrap_or("inconclusive"),
                    run.exit_code,
                    run.summary,
                    run.output_tail.join("\n"),
                ],
            )
            .map_err(db_err)?;
        self.conn
            .execute(
                "DELETE FROM machine_diagnostic_runs WHERE id NOT IN (SELECT id FROM machine_diagnostic_runs ORDER BY started_at DESC LIMIT ?1)",
                params![HISTORY_KEEP as i64],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn diagnostic_runs(&self, limit: usize) -> HubResult<Vec<RunRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, diagnostic, target, started_at, finished_at, result, exit_code, summary, output_tail FROM machine_diagnostic_runs ORDER BY started_at DESC LIMIT ?1")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let diagnostic: String = row.get(1)?;
                let tail: String = row.get(8)?;
                Ok(RunRecord {
                    id: row.get(0)?,
                    label: DiagnosticId::parse(&diagnostic)
                        .map(|d| d.label().to_string())
                        .unwrap_or_else(|| diagnostic.clone()),
                    diagnostic,
                    target: row.get(2)?,
                    started_at: row.get(3)?,
                    finished_at: row.get(4)?,
                    running: false,
                    result: RunResult::parse(&row.get::<_, String>(5)?),
                    exit_code: row.get(6)?,
                    summary: row.get(7)?,
                    output_tail: tail.lines().map(String::from).collect(),
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
}
