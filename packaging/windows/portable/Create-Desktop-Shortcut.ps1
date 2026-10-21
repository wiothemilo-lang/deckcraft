# Run once after extracting the portable ZIP. No administrator rights required.
$ErrorActionPreference = 'Stop'
$binary = Join-Path $PSScriptRoot 'DeckCraft-Fork.exe'
$launcher = Join-Path $PSScriptRoot 'Launch-DeckCraft.cmd'
if (!(Test-Path -LiteralPath $binary -PathType Leaf)) {
    throw 'Extract the complete portable ZIP before creating the shortcut.'
}
$desktop = [Environment]::GetFolderPath('DesktopDirectory')
if (!$desktop) { throw 'Windows did not provide a Desktop directory.' }
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut((Join-Path $desktop 'DeckCraft Fork.lnk'))
$shortcut.TargetPath = Join-Path $env:SystemRoot 'System32\cmd.exe'
$shortcut.Arguments = '/d /s /c ""{0}""' -f $launcher
$shortcut.WorkingDirectory = $PSScriptRoot
$shortcut.IconLocation = "$binary,0"
$shortcut.Description = 'DeckCraft fork - portable build, no Rust required'
$shortcut.WindowStyle = 7
$shortcut.Save()
$update = $shell.CreateShortcut((Join-Path $desktop 'DeckCraft Fork - Update.lnk'))
$update.TargetPath = Join-Path $env:SystemRoot 'System32\cmd.exe'
$update.Arguments = '/d /s /c ""{0}""' -f (Join-Path $PSScriptRoot 'Update-DeckCraft.cmd')
$update.WorkingDirectory = $env:TEMP
$update.IconLocation = "$binary,0"
$update.Description = 'Download a tested Windows build from wiothemilo-lang/deckcraft'
$update.WindowStyle = 7
$update.Save()
Write-Host 'Created DeckCraft Fork and DeckCraft Fork - Update on the Desktop.'
