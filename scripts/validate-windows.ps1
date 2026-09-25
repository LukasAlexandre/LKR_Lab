$ErrorActionPreference = 'Stop'
function Invoke-Gate([string]$Program, [string[]]$Arguments) {
  Write-Host "Running: $Program $($Arguments -join ' ')"
  & $Program @Arguments
  if ($LASTEXITCODE -ne 0) { throw "Gate failed: $Program $($Arguments -join ' ')" }
}
Invoke-Gate 'npm.cmd' @('ci')
Invoke-Gate 'npm.cmd' @('run','typecheck')
Invoke-Gate 'npm.cmd' @('run','lint')
Invoke-Gate 'npm.cmd' @('test')
Invoke-Gate 'npm.cmd' @('run','build')
Invoke-Gate 'cargo' @('fmt','--all','--','--check')
Invoke-Gate 'cargo' @('test','-p','hub-core')
Invoke-Gate 'cargo' @('clippy','-p','hub-core','--all-targets','--','-D','warnings')
Invoke-Gate 'cargo' @('test','-p','hub-core','live_port_detection_uses_read_only_socket','--','--ignored')
Invoke-Gate 'cargo' @('check','-p','lk-dev-hub')
Invoke-Gate 'npm.cmd' @('run','tauri','build')
Write-Host 'Automated gates complete. Run the manual smoke checklist in docs/DEVELOPMENT.md.'
