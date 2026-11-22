[CmdletBinding()]
param([ValidateSet('Build','Test','Check','Lock','Package')][string]$Mode='Build', [switch]$Offline,
      [string]$OutputDirectory)
$ErrorActionPreference='Stop'

function Assert-NoLocalBuildPaths([byte[]]$Bytes, [string[]]$Roots) {
    # A PE can contain UTF-8 paths or unaligned Windows wide strings.
    $views = @([Text.Encoding]::UTF8.GetString($Bytes))
    foreach ($offset in 0,1) {
        $views += [Text.Encoding]::Unicode.GetString($Bytes, $offset, $Bytes.Length - $offset)
    }
    foreach ($view in $views) {
        $normalized = $view.Replace('\', '/')
        foreach ($root in $Roots) {
            if ($normalized.IndexOf($root.Replace('\', '/'), [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                throw 'Release blocked: the executable contains a local build path.'
            }
        }
        # Catch a stale dependency built under a different Cargo home.
        if ($normalized -match '(?i)(?:[a-z]:/(?!/)|(?<![:/])//[^/\s]+/)[^\x00-\x1f"<>|:]{0,1024}?/registry/src/') {
            throw 'Release blocked: the executable contains an absolute Cargo registry source path.'
        }
    }
}

# Cargo needs the x64 Microsoft linker and the Windows resource compiler.
$vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw 'Install Visual Studio C++ build tools and a Windows SDK.' }
$vcvars=Join-Path $vs 'Common7/Tools/VsDevCmd.bat'
$info=[Diagnostics.ProcessStartInfo]::new()
$info.FileName=$env:ComSpec
$info.Arguments='/d /u /c "call "'+$vcvars+'" -no_logo -arch=x64 -host_arch=x64 >nul && set"'
$info.UseShellExecute=$false
$info.RedirectStandardOutput=$true
$info.StandardOutputEncoding=[Text.Encoding]::Unicode
$proc=[Diagnostics.Process]::Start($info)
$vars=$proc.StandardOutput.ReadToEnd()
$proc.WaitForExit()
if ($proc.ExitCode) { throw 'Could not initialize the Windows build tools.' }
foreach ($line in $vars -split '\r?\n') {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1],$Matches[2],'Process') }
}
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=Join-Path $env:VCToolsInstallDir 'bin/Hostx64/x64/link.exe'
$env:RC_PATH=Join-Path $env:WindowsSdkVerBinPath 'x64/rc.exe'
if (!(Get-Command cargo.exe -ErrorAction SilentlyContinue)) {
    throw 'Install Rust stable for x86_64-pc-windows-msvc and add cargo.exe to PATH.'
}
$env:CARGO_TARGET_DIR=Join-Path $PSScriptRoot 'target'
$sysroot = & rustc.exe --print sysroot
if ($LASTEXITCODE -ne 0 -or !$sysroot) { throw 'Could not locate the Rust toolchain.' }
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$mappings = @(
    @($env:USERPROFILE, '/home'),
    @($env:RUSTUP_HOME, '/rustup'),
    @($sysroot, '/rust'),
    @($cargoHome, '/cargo'),
    @($PSScriptRoot, '/source'),
    @($env:CARGO_TARGET_DIR, '/build')
)
$roots = @()
$flags = @('-Ctarget-feature=+crt-static', '-Clink-arg=/PDBALTPATH:%_PDB%')
# Rust prefers the last matching prefix. Map nested roots to the most specific name.
foreach ($mapping in ($mappings | Where-Object { $_[0] } | Sort-Object { ([IO.Path]::GetFullPath($_[0])).Length })) {
    $root = [IO.Path]::GetFullPath($mapping[0]).TrimEnd('\', '/')
    $roots += $root
    $flags += "--remap-path-prefix=$root=$($mapping[1])"
}
# Cargo's separator preserves paths with spaces as a single compiler argument.
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
Push-Location -LiteralPath $PSScriptRoot
try {
    $subcommand=@{Build='build';Test='test';Check='check';Lock='generate-lockfile';Package='build'}[$Mode]
    $cargoArgs=@($subcommand)
    if ($Mode -ne 'Lock') { $cargoArgs += @('--release','--locked') }
    if ($Offline) { $cargoArgs += '--offline' }
    & cargo.exe @cargoArgs
    if ($LASTEXITCODE) { throw "Cargo $subcommand failed ($LASTEXITCODE)." }
    if ($Mode -in 'Build','Package') {
        $binary = Get-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release/UAC2-Control.exe')
        Assert-NoLocalBuildPaths -Bytes ([IO.File]::ReadAllBytes($binary.FullName)) -Roots $roots
    }
    if ($Mode -eq 'Package') {
        $version=$binary.VersionInfo.ProductVersion
        if ($binary.VersionInfo.ProductName -cne 'UAC2 Control' -or $version -notmatch '^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.+-]+)?$') {
            throw 'The compiled executable is missing its UAC2 Control product name or version.'
        }
        $out=if($OutputDirectory){[IO.Path]::GetFullPath($OutputDirectory)}else{Join-Path $env:CARGO_TARGET_DIR 'packages'}
        [void][IO.Directory]::CreateDirectory($out)
        $package=Join-Path $out "UAC2-Control-$version-windows-x64.zip"
        $temporary=Join-Path $out ([guid]::NewGuid().ToString('N')+'.zip')
        try {
            $files=@($binary.FullName,(Join-Path $PSScriptRoot 'LICENSE'),(Join-Path $PSScriptRoot 'THIRD-PARTY-NOTICES.txt'))
            Compress-Archive -LiteralPath $files -DestinationPath $temporary -CompressionLevel Optimal
            if ([IO.File]::Exists($package)) {
                [IO.File]::Replace($temporary,$package,[System.Management.Automation.Language.NullString]::Value)
            } else {
                [IO.File]::Move($temporary,$package)
            }
            Get-Item -LiteralPath $package | Select-Object FullName,Length
        } finally {
            if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
        }
    }
    if ($Mode -eq 'Build') {
        $out=if($OutputDirectory){[IO.Path]::GetFullPath($OutputDirectory)}else{Join-Path $PSScriptRoot 'dist'}
        New-Item -ItemType Directory -Force -Path $out | Out-Null
        Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release/UAC2-Control.exe') -Destination $out
        if (!$OutputDirectory) {
            # Retire the previous build's companion only in our managed dist folder.
            $legacy=Get-Item -LiteralPath (Join-Path $out 'UAC2-AudioTest.exe') -ErrorAction SilentlyContinue
            if ($legacy -is [IO.FileInfo] -and !$legacy.LinkType -and $legacy.VersionInfo.ProductName -ceq 'UAC2 Control') {
                Remove-Item -LiteralPath $legacy.FullName
            }
        }
        Get-Item -LiteralPath (Join-Path $out 'UAC2-Control.exe') | Select-Object FullName,Length
    }
} finally { Pop-Location }
