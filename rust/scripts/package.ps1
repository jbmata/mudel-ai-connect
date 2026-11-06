param(
    [ValidateSet('debug', 'release')][string]$Profile = 'release',
    [string]$Toolchain = '',
    [string]$Target = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$manifest = Join-Path $root 'rust/Cargo.toml'
$onWindows = $env:OS -eq 'Windows_NT'
$onMac = (Test-Path -LiteralPath '/usr/bin/sw_vers') -and -not $onWindows
if (-not ($onWindows -or $onMac)) { throw 'Genera estos paquetes en Windows o macOS.' }
$cargoPrefix = @()
if ($Toolchain) { $cargoPrefix += "+$Toolchain" }
if (-not $Target) {
    $hostLine = (& rustc @cargoPrefix -vV | Select-String '^host:').ToString()
    $triple = $hostLine.Substring(6).Trim()
} else { $triple = $Target }
$architecture = $triple.Split('-')[0]
$platform = if ($onWindows) { "windows-$architecture" } else { "macos-$architecture" }
$output = Join-Path $root "dist/native/$platform"
$build = Join-Path $root "rust/target/$Profile"
$cargoTarget = @()
if ($Target) {
    $cargoTarget = @('--target', $Target)
    $build = Join-Path $root "rust/target/$Target/$Profile"
}
$cargoProfile = @()
if ($Profile -eq 'release') { $cargoProfile = @('--release') }
function Invoke-Cargo([string[]]$Arguments) {
    & cargo @cargoPrefix @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Cargo terminó con código $LASTEXITCODE." }
}
function Write-UTF8([string]$Path, [string]$Content) {
    [IO.File]::WriteAllText($Path, $Content, [Text.UTF8Encoding]::new($false))
}
function Write-Json([string]$Path, $Value) { Write-UTF8 $Path (($Value | ConvertTo-Json -Depth 30) + "`n") }
function Zip-Public([string]$Directory, [string]$Archive, [bool]$KeepParent = $false) {
    if (Test-Path -LiteralPath $Archive) { Remove-Item -LiteralPath $Archive -Force }
    if ($onMac) {
        $options = @('-c', '-k', '--sequesterRsrc')
        if ($KeepParent) { $options += '--keepParent' }
        & /usr/bin/ditto @options $Directory $Archive
        if ($LASTEXITCODE -ne 0) { throw 'No se pudo crear el ZIP de macOS.' }
    } else {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [IO.Compression.ZipFile]::CreateFromDirectory($Directory, $Archive)
    }
}

if ($onMac -and -not $env:MACOSX_DEPLOYMENT_TARGET) { $env:MACOSX_DEPLOYMENT_TARGET = '11.0' }
$suffix = if ($onWindows) { '.exe' } else { '' }
$serverName = "mudel-ai-connect-server$suffix"
$previousPayload = $env:MUDEL_SERVER_BINARY
$previousRustFlags = $env:RUSTFLAGS
try {
    if ($onWindows -and $triple.EndsWith('-msvc')) {
        # El equipo del usuario no necesita instalar el runtime de Visual C++.
        $env:RUSTFLAGS = ($env:RUSTFLAGS + ' -C target-feature=+crt-static').Trim()
        $cargoTarget = @('--target', $triple)
        $build = Join-Path $root "rust/target/$triple/$Profile"
    }
    Invoke-Cargo (@('build', '--manifest-path', $manifest, '--locked', '--bin', 'mudel-ai-connect-server') + $cargoTarget + $cargoProfile)
    $server = Join-Path $build $serverName
    $env:MUDEL_SERVER_BINARY = $server
    Invoke-Cargo (@('build', '--manifest-path', $manifest, '--locked', '--features', 'desktop', '--bin', 'mudel-ai-connect-setup') + $cargoTarget + $cargoProfile)
} finally {
    $env:MUDEL_SERVER_BINARY = $previousPayload
    $env:RUSTFLAGS = $previousRustFlags
}
$setup = Join-Path $build "mudel-ai-connect-setup$suffix"
$checkText = (& $setup --package-check | Out-String)
if ($LASTEXITCODE -ne 0) { throw 'Falló la comprobación del servidor incluido en el instalador.' }
$check = $checkText | ConvertFrom-Json
if ($check.server_sha256 -ne (Get-FileHash -LiteralPath $server -Algorithm SHA256).Hash.ToLowerInvariant()) {
    throw 'El instalador no contiene el servidor recién compilado.'
}

$stagingParent = Join-Path $root 'tmp/native-package'
$staging = Join-Path $stagingParent ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging -Force | Out-Null
New-Item -ItemType Directory -Path $output -Force | Out-Null
$published = @()
try {
    $plugin = Join-Path $staging 'plugin'
    New-Item -ItemType Directory -Path (Join-Path $plugin 'bin') -Force | Out-Null
    $sources = @{
        'LICENSE' = 'LICENSE'
        'assets/logo.png' = 'shared/assets/logo.png'
        'skills/mudel-ai-connect-study/SKILL.md' = 'local/skills/mudel-ai-connect-study/SKILL.md'
        'skills/mudel-ai-connect-study/agents/openai.yaml' = 'local/skills/mudel-ai-connect-study/agents/openai.yaml'
        'skills/setup/SKILL.md' = 'rust/plugin/skills/setup/SKILL.md'
        'skills/setup/agents/openai.yaml' = 'local/skills/setup/agents/openai.yaml'
    }
    foreach ($name in $sources.Keys) {
        $destination = Join-Path $plugin $name
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $root $sources[$name]) -Destination $destination
    }
    Copy-Item -LiteralPath $server -Destination (Join-Path $plugin "bin/$serverName")
    $metadata = Get-Content -LiteralPath (Join-Path $root 'local/plugin.json') -Raw | ConvertFrom-Json
    if ($metadata.version -ne $check.version) { throw 'La versión de Rust no coincide con la del plugin.' }
    Write-Json (Join-Path $plugin 'plugin.json') $metadata
    $claudeMetadata = Get-Content -LiteralPath (Join-Path $root 'local/.claude-plugin/plugin.json') -Raw | ConvertFrom-Json
    New-Item -ItemType Directory -Path (Join-Path $plugin '.claude-plugin') -Force | Out-Null
    Write-Json (Join-Path $plugin '.claude-plugin/plugin.json') $claudeMetadata
    foreach ($entry in @(@('mcp.json', '${PLUGIN_ROOT}'), @('.mcp.json', '${CLAUDE_PLUGIN_ROOT}'))) {
        Write-Json (Join-Path $plugin $entry[0]) @{
            mcpServers = @{ 'mudel-ai-connect' = @{ type = 'stdio'; command = "$($entry[1])/bin/$serverName"; args = @('--plugin') } }
        }
    }
    $readme = [IO.File]::ReadAllText((Join-Path $root 'rust/plugin/README.md')).Replace('__SERVER__', $serverName)
    Write-UTF8 (Join-Path $plugin 'README.md') $readme
    if ($onMac) {
        & /bin/chmod 755 (Join-Path $plugin "bin/$serverName")
        if ($LASTEXITCODE -ne 0) { throw 'No se pudieron marcar los ejecutables del plugin.' }
    }
    $pluginZip = Join-Path $output "mudel-ai-connect-plugin-native-$($metadata.version)-$platform.zip"
    Zip-Public $plugin $pluginZip
    $published += $pluginZip

    if ($onWindows) {
        $installer = Join-Path $output 'Mudel-AI-Connect-Setup.exe'
        Copy-Item -LiteralPath $setup -Destination $installer -Force
        $published += $installer
    } else {
        $app = Join-Path $staging 'Mudel AI Connect.app'
        $contents = Join-Path $app 'Contents'
        New-Item -ItemType Directory -Path (Join-Path $contents 'MacOS') -Force | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $contents 'Resources') -Force | Out-Null
        Copy-Item -LiteralPath $setup -Destination (Join-Path $contents 'MacOS/mudel-ai-connect-setup')
        Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $contents 'Resources/LICENSE')
        Write-UTF8 (Join-Path $contents 'Info.plist') @"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>mudel-ai-connect-setup</string>
<key>CFBundleIdentifier</key><string>com.mudel-ai.connect.setup</string>
<key>CFBundleName</key><string>Mudel AI Connect</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$($metadata.version)</string>
<key>CFBundleVersion</key><string>$($metadata.version)</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
"@
        & /bin/chmod 755 (Join-Path $contents 'MacOS/mudel-ai-connect-setup')
        if ($LASTEXITCODE -ne 0) { throw 'No se pudo preparar la app de macOS.' }
        & /usr/bin/codesign --force --deep --sign - $app
        if ($LASTEXITCODE -ne 0) { throw 'No se pudo aplicar la firma local de la app.' }
        & /usr/bin/codesign --verify --deep --strict $app
        if ($LASTEXITCODE -ne 0) { throw 'La firma local de la app no es válida.' }
        $installer = Join-Path $output "Mudel-AI-Connect-Setup-$platform.zip"
        Zip-Public $app $installer $true
        $published += $installer
    }
    foreach ($name in @('LICENSE', 'README.md')) {
        $destination = Join-Path $output $name
        if ($name -eq 'LICENSE') { Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination $destination -Force }
        else { Copy-Item -LiteralPath (Join-Path $root 'rust/README.md') -Destination $destination -Force }
        $published += $destination
    }
    $sums = foreach ($file in $published) { (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + (Split-Path -Leaf $file) }
    Write-UTF8 (Join-Path $output 'SHA256SUMS.txt') (($sums -join "`n") + "`n")
    Write-Output "Paquetes nativos: $output"
} finally {
    $resolvedStaging = [IO.Path]::GetFullPath($staging)
    $allowedParent = [IO.Path]::GetFullPath($stagingParent) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedStaging.StartsWith($allowedParent, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Se rechazó limpiar una ruta fuera del directorio temporal de empaquetado.'
    }
    Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
}
