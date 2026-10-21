# Offline integration checks. Creates tiny synthetic EXEs; never builds DeckCraft or downloads.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'portable\Portable-Update.Core.ps1')
Add-Type -AssemblyName System.IO.Compression.FileSystem
$scratch = Join-Path $env:TEMP ('deckcraft-updater-test-' + [Guid]::NewGuid().ToString('N'))
$root = Join-Path $scratch 'portable & spaces'
New-Item -ItemType Directory -Path $root | Out-Null
function Assert-True($Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Assert-Rejected([scriptblock]$Block, [string]$Message) {
    $rejected = $false
    try { & $Block } catch { $rejected = $true }
    Assert-True $rejected $Message
}
function New-TestPackage([string]$Commit, [string]$Name, [switch]$BadExe, [string]$Extra = '') {
    $source = Join-Path $scratch ($Name + '-source')
    New-Item -ItemType Directory -Path $source | Out-Null
    foreach ($file in $script:PortableFiles) {
        [IO.File]::WriteAllText((Join-Path $source $file), 'synthetic fixture')
    }
    if (!$BadExe) { Copy-Item -LiteralPath $probe -Destination (Join-Path $source 'DeckCraft-Fork.exe') -Force }
    @{repository=$script:PortableRepository;commit=$Commit} | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $source 'BUILD-INFO.json') -Encoding utf8
    $zipPath = Join-Path $scratch ($Name + '.zip')
    [IO.Compression.ZipFile]::CreateFromDirectory($source, $zipPath)
    if ($Extra) {
        $zip = [IO.Compression.ZipFile]::Open($zipPath, 'Update')
        try { $entry = $zip.CreateEntry($Extra); $stream=$entry.Open(); $stream.WriteByte(1); $stream.Dispose() }
        finally { $zip.Dispose() }
    }
    $manifest = @{schema=1;repository=$script:PortableRepository;channel='main';commit=$Commit;
        sha256=(Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash;size=(Get-Item -LiteralPath $zipPath).Length}
    return @{Zip=$zipPath;Manifest=$manifest}
}
try {
    $probe = Join-Path $scratch 'probe.exe'
    Add-Type -OutputAssembly $probe -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.IO;
public class DeckCraftUpdateProbe {
    public static void Main(string[] args) {
        if (args.Length == 1 && args[0] == "--version") return;
        if (args.Length < 1) Environment.Exit(2);
        string[] lines = new string[args.Length];
        lines[0] = Environment.GetEnvironmentVariable("APPDATA");
        for (int i=1; i<args.Length; i++) lines[i] = args[i];
        File.WriteAllLines(args[0], lines);
    }
}
'@
    Get-ChildItem (Join-Path $PSScriptRoot 'portable') -File | Copy-Item -Destination $root
    Copy-Item -LiteralPath $probe -Destination (Join-Path $root 'DeckCraft-Fork.exe')
    'my presentation' | Set-Content -LiteralPath (Join-Path $root 'my-talk.deckcraft')
    $one='1' * 40; $two='2' * 40; $three='3' * 40
    $good = New-TestPackage $one 'good'
    # Match the workflow's real ZIP layout, including its DeckCraft-Fork/ prefix.
    $bundleParent = Join-Path $scratch 'archive-layout'
    $bundleFolder = Join-Path $bundleParent 'DeckCraft-Fork'
    New-Item -ItemType Directory -Path $bundleFolder | Out-Null
    Get-ChildItem (Join-Path $scratch 'good-source') -File | Copy-Item -Destination $bundleFolder
    $prefixedZip = Join-Path $scratch 'prefixed.zip'
    Compress-Archive -LiteralPath $bundleFolder -DestinationPath $prefixedZip
    $prefixedManifest = $good.Manifest.Clone()
    $prefixedManifest.sha256 = (Get-FileHash -LiteralPath $prefixedZip -Algorithm SHA256).Hash
    $prefixedManifest.size = (Get-Item -LiteralPath $prefixedZip).Length
    Expand-PortablePackage $prefixedZip (Join-Path $scratch 'prefixed-out') $prefixedManifest $one
    $badHash = $good.Manifest.Clone(); $badHash.sha256='0' * 64
    Assert-Rejected { Expand-PortablePackage $good.Zip (Join-Path $scratch 'bad-hash') $badHash $one } 'Checksum mismatch was accepted.'
    $traversal = New-TestPackage $one 'traversal' -Extra '../escape.txt'
    Assert-Rejected { Expand-PortablePackage $traversal.Zip (Join-Path $scratch 'traversal-out') $traversal.Manifest $one } 'ZIP traversal was accepted.'
    Assert-True (!(Test-Path -LiteralPath (Join-Path $scratch 'escape.txt'))) 'ZIP wrote outside staging.'
    $duplicate = New-TestPackage $one 'duplicate' -Extra 'DeckCraft-Fork.exe'
    Assert-Rejected { Expand-PortablePackage $duplicate.Zip (Join-Path $scratch 'duplicate-out') $duplicate.Manifest $one } 'Duplicate ZIP entry was accepted.'
    $wrongIdentity = $good.Manifest.Clone(); $wrongIdentity.repository='other/repo'
    Assert-Rejected { Expand-PortablePackage $good.Zip (Join-Path $scratch 'wrong-repo') $wrongIdentity $one } 'Another repository was accepted.'
    $bad = New-TestPackage $two 'bad-startup' -BadExe
    $badStage = Join-Path $scratch 'bad-stage'
    Expand-PortablePackage $bad.Zip $badStage $bad.Manifest $two
    Assert-Rejected { Enable-PortableVersion $root $badStage $two } 'Invalid executable startup was accepted.'
    Assert-True ((Get-ActivePortableDirectory $root) -eq $root) 'Failed startup changed the active app.'
    $stage = Join-Path $scratch 'good-stage'
    Expand-PortablePackage $good.Zip $stage $good.Manifest $one
    Enable-PortableVersion $root $stage $one
    $first = Get-ActivePortableDirectory $root
    Assert-True ($first -eq (Join-Path $root ('.versions\' + $one))) 'Successful update did not activate.'
    $next = New-TestPackage $two 'next'
    $nextStage = Join-Path $scratch 'next-stage'
    Expand-PortablePackage $next.Zip $nextStage $next.Manifest $two
    $pointerLock = [IO.File]::Open((Join-Path $root '.current-version'), 'Open', 'Read', 'None')
    try { Assert-Rejected { Enable-PortableVersion $root $nextStage $two } 'Locked pointer replacement was accepted.' }
    finally { $pointerLock.Dispose() }
    Assert-True ((Get-ActivePortableDirectory $root) -eq $first) 'Failed activation changed the active version.'
    Assert-True (!(Test-Path -LiteralPath (Join-Path $root ('.versions\' + $two)))) 'Failed candidate was not cleaned.'
    $unowned = Join-Path $root ('.versions\' + ('4' * 40))
    New-Item -ItemType Directory -Path $unowned | Out-Null
    'user file with a package-like name' | Set-Content -LiteralPath (Join-Path $unowned 'README.txt')
    'user content' | Set-Content -LiteralPath (Join-Path $first 'keep-me.deckcraft')
    $nextStage = Join-Path $scratch 'next-stage-again'
    Expand-PortablePackage $next.Zip $nextStage $next.Manifest $two
    Enable-PortableVersion $root $nextStage $two
    Assert-True (Test-Path -LiteralPath (Join-Path $first 'keep-me.deckcraft')) 'Cleanup removed a user-added file.'
    Remove-Item -LiteralPath (Join-Path $first 'keep-me.deckcraft')
    $last = New-TestPackage $three 'last'
    $lastStage = Join-Path $scratch 'last-stage'
    Expand-PortablePackage $last.Zip $lastStage $last.Manifest $three
    Enable-PortableVersion $root $lastStage $three
    Assert-True (!(Test-Path -LiteralPath $first)) 'Unused managed version was not cleaned.'
    Assert-True (Test-Path -LiteralPath (Join-Path $root 'my-talk.deckcraft')) 'Original folder contents changed.'
    Assert-True (Test-Path -LiteralPath (Join-Path $unowned 'README.txt')) 'Unowned version directory was cleaned.'
    $stale = Join-Path $root ('.update-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stale | Out-Null
    @{repository=$script:PortableRepository;directory=[IO.Path]::GetFileName($stale)} | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $stale '.owner.json')
    'partial download' | Set-Content -LiteralPath (Join-Path $stale 'package.zip')
    Remove-PortableWork $stale $root
    Assert-True (!(Test-Path -LiteralPath $stale)) 'Owned interrupted download was not cleaned.'
    # Exercise the actual launcher, argument quoting and child-only settings after activation.
    $probeOutput = Join-Path $scratch 'launch result.txt'
    $deckArgument = Join-Path $scratch 'a presentation & spaces.pptx'
    $oldAppData = $env:APPDATA
    $command = '/d /s /c ""{0}" "{1}" "{2}""' -f (Join-Path $root 'Launch-DeckCraft.cmd'), $probeOutput, $deckArgument
    Start-Process -FilePath $env:ComSpec -ArgumentList $command -WindowStyle Hidden -Wait
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (!(Test-Path -LiteralPath $probeOutput) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    Assert-True (Test-Path -LiteralPath $probeOutput) 'Launcher did not start the active executable.'
    $lines = Get-Content -LiteralPath $probeOutput
    Assert-True ($lines[0] -eq (Join-Path $env:LOCALAPPDATA 'DeckCraft-Fork')) 'Child settings are not isolated.'
    Assert-True ($lines[1] -eq $deckArgument) 'Presentation path quoting changed.'
    Assert-True ($env:APPDATA -eq $oldAppData) 'Parent environment changed.'
    # Simulate process termination before movement, after movement, and after startup.
    foreach ($checkpoint in @('before-move', 'after-move', 'after-startup')) {
        $digit = @{ 'before-move'='5'; 'after-move'='6'; 'after-startup'='7' }[$checkpoint]
        $commit = $digit * 40
        $retry = New-TestPackage $commit ('retry-' + $checkpoint)
        $stranded = Join-Path $scratch ('stranded-' + $checkpoint)
        Expand-PortablePackage $retry.Zip $stranded $retry.Manifest $commit
        @{repository=$script:PortableRepository;commit=$commit} | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $stranded '.managed-version.json') -Encoding utf8
        if ($checkpoint -ne 'before-move') {
            $candidate = Join-Path $root ('.versions\' + $commit)
            Move-Item -LiteralPath $stranded -Destination $candidate
            if ($checkpoint -eq 'after-startup') {
                $p = Start-Process -FilePath (Join-Path $candidate 'DeckCraft-Fork.exe') -ArgumentList '--version' -WindowStyle Hidden -PassThru -Wait
                Assert-True ($p.ExitCode -eq 0) 'Synthetic startup checkpoint failed.'
            }
        }
        $retryStage = Join-Path $scratch ('retry-stage-' + $checkpoint)
        Expand-PortablePackage $retry.Zip $retryStage $retry.Manifest $commit
        Enable-PortableVersion $root $retryStage $commit
        Assert-True ((Get-ActivePortableDirectory $root) -eq (Join-Path $root ('.versions\' + $commit))) 'Interrupted update could not retry the same release.'
        # Termination after pointer publication must never cause the selected app to be deleted.
        $again = Join-Path $scratch ('active-again-' + $checkpoint)
        Expand-PortablePackage $retry.Zip $again $retry.Manifest $commit
        Assert-Rejected { Enable-PortableVersion $root $again $commit } 'Active version was replaced during retry recovery.'
        Assert-True (Test-Path -LiteralPath (Join-Path (Get-ActivePortableDirectory $root) 'DeckCraft-Fork.exe')) 'Active version was deleted.'
    }
    $protectedCommit = '8' * 40
    $protected = New-TestPackage $protectedCommit 'protected-orphan'
    $protectedStage = Join-Path $scratch 'protected-orphan-stage'
    Expand-PortablePackage $protected.Zip $protectedStage $protected.Manifest $protectedCommit
    @{repository=$script:PortableRepository;commit=$protectedCommit} | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $protectedStage '.managed-version.json') -Encoding utf8
    'user content' | Set-Content -LiteralPath (Join-Path $protectedStage 'keep.deckcraft')
    $protectedDestination = Join-Path $root ('.versions\' + $protectedCommit)
    Move-Item -LiteralPath $protectedStage -Destination $protectedDestination
    $protectedRetry = Join-Path $scratch 'protected-retry'
    Expand-PortablePackage $protected.Zip $protectedRetry $protected.Manifest $protectedCommit
    Assert-Rejected { Enable-PortableVersion $root $protectedRetry $protectedCommit } 'Recovery removed user-added orphan content.'
    Assert-True (Test-Path -LiteralPath (Join-Path $protectedDestination 'keep.deckcraft')) 'Recovery lost a user file.'
    Assert-True ((Get-ActivePortableDirectory $root) -eq (Join-Path $root ('.versions\' + ('7' * 40)))) 'Rejected recovery changed the active app.'
    [IO.File]::WriteAllText((Join-Path $root '.current-version'), '../outside')
    Assert-Rejected { Get-ActivePortableDirectory $root } 'Invalid active version pointer was accepted.'
    Write-Host 'PASS: checksum, ZIP traversal/duplicates, identity, failed startup/activation, atomic switch, interrupted-update retry, cleanup, user files and launcher arguments.'
} finally {
    # Only this invocation's generated fixtures; never the installed app or shared caches.
    $full = [IO.Path]::GetFullPath($scratch)
    $tempPrefix = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    if (!$full.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($full) -notmatch '^deckcraft-updater-test-[a-f0-9]{32}$') { throw 'Unsafe fixture cleanup path.' }
    Remove-Item -LiteralPath $full -Recurse -Force
}
