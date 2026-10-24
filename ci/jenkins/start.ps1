<#
.SYNOPSIS
Build and start local Jenkins at http://localhost:8080 (Docker Desktop required).
.DESCRIPTION
Run: powershell -ExecutionPolicy Bypass -File ci/jenkins/start.ps1
Login: admin; password is stored locally in ci/jenkins/.env (ignored by Git).
Commit and push Jenkinsfile to main before the first Build Now.
The job polls GitHub main every two minutes. Private repositories require a
read-only Jenkins Git credential added to the job's SCM configuration.
Stop with: docker compose -f ci/jenkins/compose.yaml stop
Data persists in the game-engine-ci_jenkins-home Docker volume.
This Linux CI checks headless tests and web builds; native GPU tests and Windows
desktop packaging must be exercised separately on a suitable Windows agent.
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
docker info --format '{{.OSType}}'
if ($LASTEXITCODE -ne 0) { throw 'Start Docker Desktop in Linux containers mode, then run this script again.' }
$engineType = docker info --format '{{.OSType}}'
if ($engineType -ne 'linux') { throw 'Switch Docker Desktop to Linux containers.' }
$envPath = Join-Path $PSScriptRoot '.env'
if (-not (Test-Path -LiteralPath $envPath)) {
    $randomBytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try
    {
        $rng.GetBytes($randomBytes)
    }
    finally
    {
        $rng.Dispose()
    }
    $password = [Convert]::ToBase64String($randomBytes)
    Set-Content -LiteralPath $envPath -Value "JENKINS_ADMIN_PASSWORD=$password" -Encoding ascii
}
docker compose --env-file $envPath -f (Join-Path $PSScriptRoot 'compose.yaml') up --build -d --wait --wait-timeout 180 jenkins
if ($LASTEXITCODE -ne 0) { throw 'Jenkins startup failed. Inspect the Docker Compose output above.' }
# Obtain only the inbound agent credential; never pass controller credentials to builds.
$passwordLine = Get-Content -LiteralPath $envPath | Where-Object { $_ -like 'JENKINS_ADMIN_PASSWORD=*' } | Select-Object -First 1
$password = $passwordLine.Substring('JENKINS_ADMIN_PASSWORD='.Length)
$basic = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("admin:$password"))
$jnlpContent = (Invoke-WebRequest -UseBasicParsing -Uri 'http://127.0.0.1:8080/computer/engine-linux/jenkins-agent.jnlp' -Headers @{ Authorization = "Basic $basic" }).Content
if ($jnlpContent -is [byte[]]) { $jnlpContent = [Text.Encoding]::UTF8.GetString($jnlpContent) }
# Jenkins may return XML 1.1 as binary content; .NET's XML parser supports 1.0.
$jnlp = New-Object System.Xml.XmlDocument
try { $jnlp.LoadXml(($jnlpContent -replace 'version="1\.1"', 'version="1.0"')) }
catch { throw 'Could not parse the Jenkins agent configuration response.' }
$agentSecret = [string]$jnlp.jnlp.'application-desc'.argument[0]
if ($agentSecret -notmatch '^[a-fA-F0-9]{64}$') { throw 'Jenkins returned an invalid agent credential.' }
$lines = @(Get-Content -LiteralPath $envPath | Where-Object { $_ -notlike 'JENKINS_AGENT_SECRET=*' })
$lines += "JENKINS_AGENT_SECRET=$agentSecret"
Set-Content -LiteralPath $envPath -Value $lines -Encoding ascii
docker compose --env-file $envPath -f (Join-Path $PSScriptRoot 'compose.yaml') up --build -d agent
if ($LASTEXITCODE -ne 0) { throw 'Jenkins agent startup failed.' }
Write-Host 'Jenkins is ready at http://localhost:8080. Login: admin.'
Write-Host "Password file: $envPath"
Write-Host 'After pushing Jenkinsfile to main, open game-engine-master and select Build Now.'
