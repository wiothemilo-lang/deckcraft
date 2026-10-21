param([string]$InstallRoot = $PSScriptRoot)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Portable-Update.Core.ps1')
Add-Type -AssemblyName System.Windows.Forms
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$work = $null; $lock = $null
try {
    $InstallRoot = (Resolve-Path -LiteralPath $InstallRoot).Path
    Assert-PlainDirectory $InstallRoot
    try { $lock = [IO.File]::Open((Join-Path $InstallRoot '.update.lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
    catch { throw 'Another updater is already running, or this folder is not writable.' }
    $active = Get-ActivePortableDirectory $InstallRoot
    $current = Get-Content -LiteralPath (Join-Path $active 'BUILD-INFO.json') -Raw | ConvertFrom-Json
    if ($current.repository -cne $script:PortableRepository) { throw 'This updater only supports wiothemilo-lang/deckcraft.' }
    Clear-PortableInterruptedDownloads $InstallRoot
    $drive = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($InstallRoot))
    if ($drive.AvailableFreeSpace -lt 1GB) { throw 'Keep at least 1 GB free before updating.' }
    $work = Join-Path $InstallRoot ('.update-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    @{repository=$script:PortableRepository;directory=[IO.Path]::GetFileName($work)} | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $work '.owner.json') -Encoding utf8
    Write-Host 'Checking published Windows builds from wiothemilo-lang/deckcraft...'
    $releaseFile = Join-Path $work 'releases.json'
    Receive-PortableFile 'https://api.github.com/repos/wiothemilo-lang/deckcraft/releases?per_page=20' $releaseFile 2MB
    $releases = @(Get-Content -LiteralPath $releaseFile -Raw | ConvertFrom-Json)
    $release = $releases | Where-Object { !$_.draft -and $_.tag_name -cmatch '^fork-windows-[a-f0-9]{40}$' } |
        Sort-Object { [DateTime]$_.published_at } -Descending | Select-Object -First 1
    if (!$release) { throw 'No published fork update is available yet. Please try again after the main build finishes.' }
    $commit = $release.tag_name.Substring(13)
    if ($current.commit -ceq $commit) {
        [Windows.Forms.MessageBox]::Show('You already have the latest published Windows build.', 'DeckCraft Fork Update') | Out-Null
        return
    }
    while (Test-PortableRunning $InstallRoot) {
        $answer = [Windows.Forms.MessageBox]::Show('Save your presentation and close DeckCraft Fork, then click Retry. The updater will not close it for you.', 'DeckCraft Fork Update', 'RetryCancel', 'Information')
        if ($answer -ne 'Retry') { return }
    }
    $baseUrl = 'https://github.com/wiothemilo-lang/deckcraft/releases/download/' + $release.tag_name + '/'
    foreach ($name in @('update.json', 'DeckCraft-Fork-Windows-x64.zip')) {
        $asset = @($release.assets | Where-Object { $_.name -ceq $name -and $_.state -eq 'uploaded' })
        if ($asset.Count -ne 1 -or $asset[0].browser_download_url -cne ($baseUrl + $name)) { throw 'Release is incomplete or has an unexpected download address.' }
    }
    $manifestPath = Join-Path $work 'update.json'
    Receive-PortableFile ($baseUrl + 'update.json') $manifestPath 16KB
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $zipPath = Join-Path $work 'package.zip'
    Write-Host 'Downloading the prebuilt app. Rust/Cargo is not required. This can take a few minutes.'
    Receive-PortableFile ($baseUrl + 'DeckCraft-Fork-Windows-x64.zip') $zipPath 128MB
    $package = Join-Path $work 'package'
    Expand-PortablePackage $zipPath $package $manifest $commit
    Enable-PortableVersion $InstallRoot $package $commit
    [Windows.Forms.MessageBox]::Show('Update installed. Open DeckCraft Fork using your usual shortcut.', 'DeckCraft Fork Update') | Out-Null
} catch {
    [Windows.Forms.MessageBox]::Show(('Update was not completed. Your current app and presentations are kept.' + [Environment]::NewLine + $_.Exception.Message), 'DeckCraft Fork Update', 'OK', 'Error') | Out-Null
} finally {
    if ($work -and (Test-Path -LiteralPath $work)) {
        try {
            Remove-PortableWork $work $InstallRoot
        } catch { Write-Warning 'Some temporary update files could not be cleaned.' }
    }
    if ($lock) { $lock.Dispose() }
}
