use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: String,
    /// Vínculo desta máquina (tabela project_bindings); "" = ainda não localizado aqui.
    /// Nunca é gravado no cadastro portátil nem usado como identidade.
    #[serde(default)]
    pub local_path: String,
    pub repository: String,
    pub stack: Vec<String>,
    pub tags: Vec<String>,
    pub ports: Vec<ProjectPort>,
    pub commands: Vec<ProjectCommand>,
    pub created_at: String,
    pub updated_at: String,
}
/// Situação do projeto nesta máquina, recalculada a cada leitura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Location {
    /// Vinculado a uma pasta que existe aqui.
    Available,
    /// Vinculado, mas a pasta não existe mais nesta máquina.
    Missing,
    /// Faz parte do workspace, mas ainda não foi localizado nesta máquina.
    Unbound,
}
/// Projeto como a interface o vê: cadastro + observações desta máquina.
/// Só é serializado para o renderer; nunca volta ao banco.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    #[serde(flatten)]
    pub project: Project,
    pub location: Location,
}
/// Resultado de "Localizar": vincula ou pede confirmação explícita.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindResult {
    pub bound: bool,
    pub needs_confirmation: bool,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub name: String,
    pub description: String,
    /// Vazio ao editar um projeto ainda não localizado: mantém o vínculo como está.
    pub local_path: String,
    pub repository: String,
    pub stack: Vec<String>,
    pub tags: Vec<String>,
    pub ports: Vec<ProjectPort>,
    pub commands: Vec<ProjectCommand>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectPort {
    pub name: String,
    pub port: u16,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectCommand {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub category: String,
    pub project_id: Option<String>,
    pub body: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: i64,
    pub project_id: Option<String>,
    pub action: String,
    pub created_at: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEntry {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub kind: String,
    pub body: String,
    pub tags: String,
    #[serde(default)]
    pub updated_at: String,
}
