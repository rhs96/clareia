$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$a=$env:CLAREIA_ARGS | ConvertFrom-Json
$action=$env:CLAREIA_ACTION
$store=Join-Path $env:LOCALAPPDATA 'Clareia'
function Output($value){$value | ConvertTo-Json -Depth 12 -Compress}
function Inventory {
 $roots=@('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*')
 @(foreach($root in $roots){Get-ItemProperty $root -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -and -not $_.SystemComponent } | ForEach-Object {
  @{id=$_.PSPath;name=$_.DisplayName;version=$_.DisplayVersion;publisher=$_.Publisher;location=$_.InstallLocation;uninstall=$_.UninstallString;key=$_.PSChildName;source='Windows'}
 }})
}
function App($id){$item=@(Inventory | Where-Object id -eq $id);if($item.Count -ne 1){throw 'Aplicativo não encontrado. Atualize a lista.'};$item[0]}
function ScoopRoot {if($env:SCOOP){$env:SCOOP}else{Join-Path $env:USERPROFILE 'scoop'}}
function ScoopExe {$path=Join-Path (ScoopRoot) 'shims\scoop.ps1';if(-not(Test-Path -LiteralPath $path)){throw 'Scoop não encontrado para este usuário.'};$path}
function Package($name){if($name -notmatch '^[a-zA-Z0-9][a-zA-Z0-9._-]*(/[a-zA-Z0-9][a-zA-Z0-9._-]*)?$'){throw 'Nome de pacote inválido.'};[string]$name}
function ScoopList {
 $root=ScoopRoot
 @(Get-ChildItem -LiteralPath (Join-Path $root 'apps') -Directory -ErrorAction SilentlyContinue | Where-Object Name -ne 'scoop' | ForEach-Object {
  $m=Join-Path $_.FullName 'current\manifest.json';$i=Join-Path $_.FullName 'current\install.json'
  if(Test-Path -LiteralPath $m){$manifest=Get-Content -LiteralPath $m -Raw | ConvertFrom-Json;$info=$null;if(Test-Path -LiteralPath $i){$info=Get-Content -LiteralPath $i -Raw | ConvertFrom-Json};@{name=$_.Name;version=$manifest.version;bucket=$info.bucket;description=$manifest.description;source='Scoop'}}
 })
}
function RunScoop($verb,$package){$exe=ScoopExe;$output=(& $exe $verb $package *>&1 | Out-String);if($LASTEXITCODE -and $LASTEXITCODE -ne 0){throw $output};@{message='Operação Scoop concluída. Confira o log.';log=$output}}
function Receipt($id){if($id -notmatch '^[0-9a-f-]{36}$'){throw 'Registro inválido'};$path=Join-Path $store "receipts\$id.json";if(-not(Test-Path -LiteralPath $path)){throw 'Registro não encontrado'};Get-Content -LiteralPath $path -Raw | ConvertFrom-Json}
function SaveReceipt($app){$id=[guid]::NewGuid().ToString();New-Item -ItemType Directory -Force (Join-Path $store 'receipts')|Out-Null;@{id=$id;name=$app.name;created=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $store "receipts\$id.json");$id}
function SafePath($path){
 $full=[IO.Path]::GetFullPath($path).TrimEnd('\');$allowed=@($env:LOCALAPPDATA,$env:APPDATA,$env:TEMP) | ForEach-Object {[IO.Path]::GetFullPath($_).TrimEnd('\')}
 if(-not @($allowed | Where-Object {$full.StartsWith($_+'\',[StringComparison]::OrdinalIgnoreCase)}).Count){throw 'Caminho fora das pastas permitidas.'}
 if($full.StartsWith([IO.Path]::GetFullPath($store),[StringComparison]::OrdinalIgnoreCase)){throw 'Pasta do Clareia protegida.'}
 $cursor=$full
 while($cursor -and (Test-Path -LiteralPath $cursor)){$item=Get-Item -LiteralPath $cursor -Force;if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Links e junções não são movidos pela limpeza.'};$parent=Split-Path $cursor -Parent;if($parent -eq $cursor){break};$cursor=$parent}
 $full
}
function RejectNestedLinks($path){
 $stack=New-Object 'System.Collections.Generic.Stack[string]';$stack.Push($path)
 while($stack.Count){$next=$stack.Pop();foreach($child in @(Get-ChildItem -LiteralPath $next -Force -ErrorAction Stop)){if($child.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'A pasta contém links ou junções. Use a revisão manual.'};if($child.PSIsContainer){$stack.Push($child.FullName)}}}
}
function Candidates($receipt){
 $name=([string]$receipt.name -replace '[<>:"/\\|?*]','').Trim();if($name.Length -lt 4){return @()}
 $names=@($name,($name -replace '\s+\([^)]+\)$','')) | Select-Object -Unique
 @(foreach($root in @($env:LOCALAPPDATA,$env:APPDATA,$env:TEMP)){foreach($n in $names){$p=Join-Path $root $n;if(Test-Path -LiteralPath $p){try{$safe=SafePath $p;$item=Get-Item -LiteralPath $safe -Force;@{path=$safe;stamp=$item.LastWriteTimeUtc.Ticks.ToString();reason='Nome exato do aplicativo. Pode conter configurações ou dados pessoais; revisão obrigatória.'}}catch{}}}})
}
function WindowsScan {
 $session=New-Object -ComObject Microsoft.Update.Session;$search=$session.CreateUpdateSearcher().Search("IsInstalled=0 and IsHidden=0 and Type='Software'")
 @{session=$session;collection=$search.Updates;items=@(for($i=0;$i -lt $search.Updates.Count;$i++){$u=$search.Updates.Item($i);@{id=$u.Identity.UpdateID;title=$u.Title;bytes=[double]$u.MaxDownloadSize;downloaded=$u.IsDownloaded;eulaAccepted=$u.EulaAccepted}})}
}
try{
 switch($action){
 'inventory' {Output @{items=@(Inventory)}}
 'scoop_list' {Output @{available=(Test-Path -LiteralPath (Join-Path (ScoopRoot) 'shims\scoop.ps1'));items=@(ScoopList)}}
 'scoop_search' {
  $q=[string]$a.query;if($q.Length -lt 2){throw 'Digite pelo menos dois caracteres.'};$items=@()
  $buckets=Join-Path (ScoopRoot) 'buckets'
  foreach($bucket in @(Get-ChildItem -LiteralPath $buckets -Directory -ErrorAction SilentlyContinue)){
   foreach($file in @(Get-ChildItem -LiteralPath (Join-Path $bucket.FullName 'bucket') -Filter '*.json' -File -ErrorAction SilentlyContinue | Where-Object {$_.BaseName.IndexOf($q,[StringComparison]::OrdinalIgnoreCase) -ge 0} | Select-Object -First 100)){
    $m=Get-Content -LiteralPath $file.FullName -Raw|ConvertFrom-Json;$items+=@{name=$file.BaseName;package="$($bucket.Name)/$($file.BaseName)";version=$m.version;description=$m.description;bucket=$bucket.Name}
   }
  };Output @{items=@($items | Select-Object -First 200);note='Busca nos buckets já instalados. Atualize os catálogos para obter novas versões.'}
 }
 'scoop_status' {$exe=ScoopExe;$log=(& $exe status *>&1|Out-String);Output @{log=$log}}
 'prepare_scoop_refresh' {Output @{title='Atualizar catálogos Scoop';description='Buscar alterações do próprio Scoop e dos buckets instalados. Não atualiza todos os aplicativos.';args=@{}}}
 {$_ -in 'prepare_scoop_install','prepare_scoop_update','prepare_scoop_uninstall'} {
  $name=Package $a.package;$verb=$action.Replace('prepare_scoop_','');$null=ScoopExe
  if($verb -ne 'install' -and -not @(ScoopList | Where-Object name -eq $name).Count){throw 'Pacote instalado não encontrado.'}
  Output @{title="Scoop · $verb";description="Executar scoop $verb $name. O Scoop executará os scripts oficiais do bucket configurado. A desinstalação padrão preserva a pasta persist quando aplicável.";details=@($name);args=@{package=$name}}
 }
 'scoop_refresh' {$exe=ScoopExe;$log=(& $exe update *>&1|Out-String);if($LASTEXITCODE -and $LASTEXITCODE -ne 0){throw $log};Output @{message='Catálogos atualizados.';log=$log}}
 'scoop_install' {Output (RunScoop 'install' (Package $a.package))}
 'scoop_update' {Output (RunScoop 'update' (Package $a.package))}
 'scoop_uninstall' {Output (RunScoop 'uninstall' (Package $a.package))}
 'prepare_uninstall' {
  $app=App $a.id;if(-not $app.uninstall){throw 'O aplicativo não registrou um desinstalador. Verifique o Scoop ou o instalador original.'}
  Output @{title="Desinstalar $($app.name)";description='Abrir o desinstalador registrado pelo aplicativo. A janela oficial pode pedir outras confirmações. Após o término, o Clareia verifica se o registro foi removido e oferece a busca de resíduos.';details=@($app.name,$app.publisher,$app.uninstall);args=@{id=$app.id;expected=$app.uninstall}}
 }
 'uninstall' {
  $app=App $a.id;if($app.uninstall -ne $a.expected){throw 'O desinstalador mudou. Revise novamente.'};$line=[Environment]::ExpandEnvironmentVariables([string]$app.uninstall)
  if($line -match '^\s*"([^"]+\.exe)"\s*(.*)$'){$exe=$matches[1];$tail=$matches[2]}
  elseif($line -match '^\s*(.+?\.exe)\s*(.*)$'){$exe=$matches[1];$tail=$matches[2]}else{throw 'Formato de desinstalação não suportado. Use Aplicativos instalados do Windows.'}
  $base=[IO.Path]::GetFileName($exe)
  if($base -ieq 'msiexec.exe'){
   if($app.key -notmatch '^\{[0-9A-Fa-f-]{36}\}$'){throw 'Identificador MSI não encontrado.'};$exe=Join-Path $env:SystemRoot 'System32\msiexec.exe';$tail="/x $($app.key)"
  }else{
   if(-not [IO.Path]::IsPathRooted($exe) -or -not(Test-Path -LiteralPath $exe -PathType Leaf)){throw 'Desinstalador não encontrado.'}
   if($base -imatch '^(cmd|powershell|pwsh|wscript|cscript|rundll32|reg|mshta)\.exe$'){throw 'Desinstalador por interpretador não é executado por este painel.'}
  }
  $start=@{FilePath=$exe;PassThru=$true};if($tail){$start.ArgumentList=$tail};$p=Start-Process @start;$p.WaitForExit()
  if(@(Inventory|Where-Object id -eq $app.id).Count){Output @{message='O desinstalador terminou, mas o aplicativo continua registrado. Pode ter sido cancelado ou exigir reinício.';removed=$false}}
  else{$id=SaveReceipt $app;Output @{message='Registro de instalação removido. A busca de resíduos está disponível.';removed=$true;receipt=$id}}
 }
 'residues' {$receipt=Receipt $a.receipt;Output @{name=$receipt.name;items=@(Candidates $receipt)}}
 'prepare_quarantine' {
  $receipt=Receipt $a.receipt;$candidates=@(Candidates $receipt);$selected=@($a.paths);if(-not $selected.Count){throw 'Selecione os caminhos revisados.'}
  $items=@(foreach($path in $selected){$match=@($candidates|Where-Object path -eq $path);if($match.Count -ne 1){throw 'A lista de resíduos mudou. Faça outra busca.'};$match[0]})
  Output @{title="Recolher resíduos de $($receipt.name)";description='Mover somente os caminhos selecionados para uma quarentena local, com opção de restaurar. Eles podem conter configurações e dados pessoais. Nenhuma chave de registro será apagada.';details=@($items|ForEach-Object path);args=@{receipt=$a.receipt;items=$items}}
 }
 'quarantine' {
  $null=Receipt $a.receipt;$items=@($a.items);$id=[guid]::NewGuid().ToString();$destination=Join-Path $store "quarantine\$id";New-Item -ItemType Directory -Force $destination|Out-Null
  $moved=@();$errors=@();$index=0; $journal=Join-Path $destination "manifest.json"
  foreach($item in $items){try{$source=SafePath $item.path;$actual=Get-Item -LiteralPath $source -Force;if($actual.LastWriteTimeUtc.Ticks.ToString() -ne $item.stamp){throw 'Pasta alterada desde a revisão.'};RejectNestedLinks $source;$target=Join-Path $destination ([string]$index);$index++;
   $entry=@{original=$source;saved=$target};$prospective=@($moved)+@($entry)
   @{id=$id;created=[DateTime]::UtcNow.ToString('o');items=$prospective;restored=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $journal -ErrorAction Stop
   Move-Item -LiteralPath $source -Destination $target -ErrorAction Stop;$moved+=$entry}catch{$errors+="$($item.path): $($_.Exception.Message)"}}
  @{id=$id;created=[DateTime]::UtcNow.ToString('o');items=$moved;restored=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $destination 'manifest.json')
  Output @{message="$($moved.Count) caminhos movidos para quarentena.";errors=$errors}
 }
 'quarantine_list' {
  $root=Join-Path $store 'quarantine';$items=@(Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue|ForEach-Object{$m=Join-Path $_.FullName 'manifest.json';if(Test-Path -LiteralPath $m){Get-Content -LiteralPath $m -Raw|ConvertFrom-Json}});Output @{items=$items}
 }
 'prepare_restore' {
  $id=[string]$a.id;if($id -notmatch '^[0-9a-f-]{36}$'){throw 'Quarentena inválida'};$path=Join-Path $store "quarantine\$id\manifest.json";$m=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json;if($m.restored){throw 'Já restaurada'}
  Output @{title='Restaurar quarentena';description='Repor as pastas nos locais originais. Caminhos existentes não serão sobrescritos.';details=@($m.items|ForEach-Object original);args=@{id=$id}}
 }
 'restore' {
  $id=[string]$a.id;if($id -notmatch '^[0-9a-f-]{36}$'){throw 'Quarentena inválida'};$root=Join-Path $store "quarantine\$id";$path=Join-Path $root 'manifest.json';$m=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
  if($m.restored){throw 'Já restaurada'}
  foreach($item in $m.items){$original=SafePath $item.original;$saved=[IO.Path]::GetFullPath($item.saved);if(-not $saved.StartsWith([IO.Path]::GetFullPath($root)+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Backup inválido'};if(Test-Path -LiteralPath $original){throw "O caminho já existe: $original"};if(-not(Test-Path -LiteralPath $saved)){throw 'Backup não encontrado'}}
  foreach($item in @($m.items)){Move-Item -LiteralPath $item.saved -Destination $item.original -ErrorAction Stop;$m.items=@($m.items|Where-Object saved -ne $item.saved);$m|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $path};$m.restored=$true;$m|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $path;Output @{message='Pastas restauradas.'}
 }
 'windows_scan' {$s=WindowsScan;Output @{items=$s.items}}
 'prepare_windows_install' {
  $ids=@($a.ids);if(-not $ids.Count){throw 'Selecione as atualizações.'};foreach($id in $ids){if($id -notmatch '^[0-9a-fA-F-]{36}$'){throw 'Identificador inválido'}}
  $s=WindowsScan;$updates=@($s.items|Where-Object {$ids -contains $_.id});if($updates.Count -ne $ids.Count){throw 'A lista de atualizações mudou. Faça nova verificação.'}
  Output @{title='Instalar atualizações do Windows';description='Baixar e instalar somente as atualizações selecionadas. Ao confirmar, você também aceita os termos de licença dessas atualizações da Microsoft. Pode ser necessário reiniciar; o Clareia não reiniciará automaticamente.';details=@($updates|ForEach-Object title);args=@{ids=$ids}}
 }
 'windows_install' {
  $s=WindowsScan;$selected=New-Object -ComObject Microsoft.Update.UpdateColl
  for($i=0;$i -lt $s.collection.Count;$i++){$u=$s.collection.Item($i);if(@($a.ids) -contains $u.Identity.UpdateID){if(-not $u.EulaAccepted){$u.AcceptEula()};$null=$selected.Add($u)}}
  if($selected.Count -ne @($a.ids).Count){throw 'A lista mudou. Revise novamente.'}
  $downloader=$s.session.CreateUpdateDownloader();$downloader.Updates=$selected;$download=$downloader.Download();if($download.ResultCode -ne 2){throw "Download não concluído: código $($download.ResultCode)"}
  $installer=$s.session.CreateUpdateInstaller();$installer.Updates=$selected;$result=$installer.Install();$items=@(for($i=0;$i -lt $selected.Count;$i++){ @{title=$selected.Item($i).Title;result=$result.GetUpdateResult($i).ResultCode;hresult=$result.GetUpdateResult($i).HResult} })
  Output @{message="Operação concluída com código $($result.ResultCode). Código 2 indica sucesso; 3 indica sucesso parcial.";rebootRequired=$result.RebootRequired;items=$items}
 }
 default {throw 'Ferramenta não permitida.'}
 }
}catch{[Console]::Error.WriteLine($_.Exception.Message);exit 1}


