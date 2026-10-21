param([switch]$Update)
$ErrorActionPreference = 'Stop'
$launchLock = $null
try {
    . (Join-Path $PSScriptRoot 'Portable-Update.Core.ps1')
    if (!$Update) {
        try { $launchLock = [IO.File]::Open((Join-Path $PSScriptRoot '.update.lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
        catch { throw 'An update is running, or this folder is not writable. Try again after the update finishes.' }
    }
    $active = Get-ActivePortableDirectory $PSScriptRoot
    if ($Update) {
        $scriptPath = Join-Path $active 'Update-DeckCraft.ps1'
        $arguments = '-NoProfile -ExecutionPolicy Bypass -File {0} -InstallRoot {1}' -f (
            ConvertTo-NativeArgument $scriptPath), (ConvertTo-NativeArgument $PSScriptRoot)
        Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList $arguments -WorkingDirectory $env:TEMP
    } else {
        $env:APPDATA = Join-Path $env:LOCALAPPDATA 'DeckCraft-Fork'
        $arguments = @($args | ForEach-Object { ConvertTo-NativeArgument $_ }) -join ' '
        $start = @{FilePath=(Join-Path $active 'DeckCraft-Fork.exe');WorkingDirectory=$active}
        if ($arguments) { $start.ArgumentList = $arguments }
        Start-Process @start
    }
} catch {
    Add-Type -AssemblyName System.Windows.Forms
    [Windows.Forms.MessageBox]::Show($_.Exception.Message, 'DeckCraft Fork', 'OK', 'Error') | Out-Null
    exit 1
} finally { if ($launchLock) { $launchLock.Dispose() } }
