$ErrorActionPreference='Stop'
$script=Get-Content (Join-Path $PSScriptRoot '..\src-tauri\src\tools.ps1') -Raw
$profileRoot=Join-Path ([IO.Path]::GetTempPath()) ('Clareia-test-'+[guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force "$profileRoot\local","$profileRoot\roaming","$profileRoot\temp","$profileRoot\local\FixtureTool","$profileRoot\local\Clareia\receipts"|Out-Null
Set-Content -LiteralPath "$profileRoot\local\FixtureTool\data.txt" 'conteudo preservado'
$id=[guid]::NewGuid().ToString()
@{id=$id;name='FixtureTool'}|ConvertTo-Json|Set-Content -LiteralPath "$profileRoot\local\Clareia\receipts\$id.json"
function CallTool($action,$argsValue){
 $start=New-Object Diagnostics.ProcessStartInfo
 $start.FileName="$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe"
 $start.Arguments='-NoProfile -NonInteractive -Command & ([ScriptBlock]::Create($env:CLAREIA_SCRIPT))'
 $start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$start.CreateNoWindow=$true
 $start.EnvironmentVariables['LOCALAPPDATA']="$profileRoot\local";$start.EnvironmentVariables['APPDATA']="$profileRoot\roaming";$start.EnvironmentVariables['TEMP']="$profileRoot\temp"
 $start.EnvironmentVariables['CLAREIA_SCRIPT']=$script;$start.EnvironmentVariables['CLAREIA_ACTION']=$action;$start.EnvironmentVariables['CLAREIA_ARGS']=($argsValue|ConvertTo-Json -Depth 8 -Compress)
 $p=[Diagnostics.Process]::Start($start);$output=$p.StandardOutput.ReadToEnd();$errorText=$p.StandardError.ReadToEnd();$p.WaitForExit();if($p.ExitCode -ne 0){throw $errorText};$output|ConvertFrom-Json
}
$r=CallTool 'residues' @{receipt=$id};if($r.items.Count -ne 1){throw 'Candidato não encontrado'}
$plan=CallTool 'prepare_quarantine' @{receipt=$id;paths=@($r.items[0].path)}
$null=CallTool 'quarantine' $plan.args
if(Test-Path -LiteralPath "$profileRoot\local\FixtureTool"){throw 'Pasta não foi movida'}
$q=CallTool 'quarantine_list' @{}
$restore=CallTool 'prepare_restore' @{id=$q.items[-1].id}
$null=CallTool 'restore' $restore.args
if((Get-Content -LiteralPath "$profileRoot\local\FixtureTool\data.txt") -ne 'conteudo preservado'){throw 'Falha na restauração'}
$rejected=$false;try{CallTool 'prepare_quarantine' @{receipt=$id;paths=@('C:\Windows')}|Out-Null}catch{$rejected=$true};if(-not $rejected){throw 'Caminho indevido não foi rejeitado'}
'PASS: busca conservadora, revisão, quarentena, restauração e rejeição de caminho externo'


