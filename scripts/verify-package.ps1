# Comprueba los paquetes públicos de dist/native/<plataforma> sin una cuenta real:
# contenido del plugin, lanzador nativo, servidor incluido en el instalador y huellas.
param([string]$Platform = '')

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$onWindows = $env:OS -eq 'Windows_NT'
if (-not $Platform) {
    $arch = if ($onWindows) { ($env:PROCESSOR_ARCHITECTURE -replace 'AMD64', 'x86_64' -replace 'ARM64', 'aarch64').ToLowerInvariant() }
            else { (& /usr/bin/uname -m).Trim() -replace 'arm64', 'aarch64' }
    $Platform = $(if ($onWindows) { 'windows' } else { 'macos' }) + "-$arch"
}
$directory = Join-Path $root "dist/native/$Platform"
if (-not (Test-Path -LiteralPath $directory)) { throw "Genera primero los paquetes con scripts/package.ps1 ($directory)." }
$server = 'mudel-ai-connect-server' + $(if ($onWindows) { '.exe' } else { '' })
$failures = New-Object System.Collections.Generic.List[string]
function Check([bool]$Condition, [string]$Message) {
    if ($Condition) { Write-Output "  ok   $Message" } else { Write-Output "  FALLO $Message"; $failures.Add($Message) }
}
function Read-Entry($Archive, [string]$Name) {
    $entry = $Archive.GetEntry($Name)
    if (-not $entry) { throw "Falta $Name en el ZIP." }
    $stream = $entry.Open(); $memory = New-Object IO.MemoryStream
    try { $stream.CopyTo($memory) } finally { $stream.Dispose() }
    , $memory.ToArray()
}

Write-Output "Plugin ($Platform)"
$plugins = @(Get-ChildItem -LiteralPath $directory -Filter 'mudel-ai-connect-plugin-native-*.zip')
Check ($plugins.Count -eq 1) 'hay un único ZIP del plugin'
$pluginBytes = $null
$archive = [IO.Compression.ZipFile]::OpenRead($plugins[0].FullName)
try {
    $names = @($archive.Entries | Where-Object { $_.FullName -notmatch '/$' -and $_.FullName -notmatch '^__MACOSX/' } |
        ForEach-Object { $_.FullName } | Sort-Object)
    $expected = @('LICENSE', 'README.md', 'assets/logo.png', 'plugin.json', 'mcp.json', '.mcp.json',
        '.claude-plugin/plugin.json', "bin/$server", 'skills/setup/SKILL.md', 'skills/setup/agents/openai.yaml',
        'skills/mudel-ai-connect-study/SKILL.md', 'skills/mudel-ai-connect-study/agents/openai.yaml') | Sort-Object
    Check (($names -join '|') -ceq ($expected -join '|')) 'contiene solo los archivos públicos esperados'
    foreach ($pair in @(@('mcp.json', '${PLUGIN_ROOT}'), @('.mcp.json', '${CLAUDE_PLUGIN_ROOT}'))) {
        $json = [Text.Encoding]::UTF8.GetString((Read-Entry $archive $pair[0])) | ConvertFrom-Json
        $entry = $json.mcpServers.'mudel-ai-connect'
        Check ($entry.command -ceq "$($pair[1])/bin/$server" -and ($entry.args -join ' ') -ceq '--plugin' -and $entry.type -ceq 'stdio') "$($pair[0]) usa el lanzador nativo con --plugin"
    }
    $pluginBytes = Read-Entry $archive "bin/$server"
    Check ($pluginBytes.Length -gt 1024) 'el servidor del plugin no está vacío'
    if (-not $onWindows) {
        $mode = $archive.GetEntry("bin/$server").ExternalAttributes -shr 16
        Check (($mode -band 73) -ne 0) 'el servidor del plugin es ejecutable'
    }
} finally { $archive.Dispose() }

Write-Output 'Instalador'
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('mudel-verify-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    if ($onWindows) {
        $setup = Join-Path $directory 'Mudel-AI-Connect-Setup.exe'
    } else {
        $app = Get-ChildItem -LiteralPath $directory -Filter 'Mudel-AI-Connect-Setup-*.zip' | Select-Object -First 1
        & /usr/bin/ditto -x -k $app.FullName $temporary
        if ($LASTEXITCODE -ne 0) { throw 'No se pudo extraer la app de macOS.' }
        $setup = Join-Path $temporary 'Mudel AI Connect.app/Contents/MacOS/mudel-ai-connect-setup'
        Check ((Get-Content -LiteralPath (Join-Path $temporary 'Mudel AI Connect.app/Contents/Info.plist') -Raw) -match 'CFBundleExecutable') 'la app tiene Info.plist'
    }
    $text = (& $setup --package-check | Out-String)
    Check ($LASTEXITCODE -eq 0) 'el instalador ejecuta --package-check'
    $check = $text | ConvertFrom-Json
    $sha = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($pluginBytes)).Replace('-', '').ToLowerInvariant()
    Check ($check.server_bytes -eq $pluginBytes.Length) 'el servidor incluido pesa lo mismo que el del plugin'
    Check ($check.server_sha256 -ceq $sha) 'el servidor incluido coincide por SHA-256 con el del plugin'
    $metadata = Get-Content -LiteralPath (Join-Path $root 'plugin/metadata/plugin.json') -Raw | ConvertFrom-Json
    Check ($check.version -ceq $metadata.version) 'la versión del instalador coincide con la del plugin'
} finally {
    $resolved = [IO.Path]::GetFullPath($temporary)
    if ($resolved.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

Write-Output 'Huellas'
$sums = @(Get-Content -LiteralPath (Join-Path $directory 'SHA256SUMS.txt') | Where-Object { $_ })
Check ($sums.Count -eq 4) 'SHA256SUMS.txt cubre los cuatro archivos publicados'
foreach ($line in $sums) {
    $digest, $name = $line -split '  ', 2
    Check ((Split-Path -Leaf $name) -ceq $name) "$name no sale del paquete"
    $actual = (Get-FileHash -LiteralPath (Join-Path $directory $name) -Algorithm SHA256).Hash.ToLowerInvariant()
    Check ($actual -ceq $digest) "huella correcta de $name"
}

if ($failures.Count -gt 0) { Write-Output "$($failures.Count) comprobación(es) fallaron."; exit 1 }
Write-Output 'Paquetes verificados.'
