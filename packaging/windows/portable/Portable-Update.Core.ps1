# Shared by the launcher, updater and offline integration checks. Windows PowerShell 5.1.
$script:PortableRepository = 'wiothemilo-lang/deckcraft'
$script:PortableFiles = @('DeckCraft-Fork.exe', 'BUILD-INFO.json', 'Launch-DeckCraft.cmd',
    'Start-Portable.ps1', 'Update-DeckCraft.cmd', 'Update-DeckCraft.ps1', 'Portable-Update.Core.ps1',
    'Create-Desktop-Shortcut.cmd', 'Create-Desktop-Shortcut.ps1', 'README.txt',
    'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md')

function Assert-PlainDirectory([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'The portable folder must be a regular directory.'
    }
}

function Get-ActivePortableDirectory([string]$InstallRoot) {
    Assert-PlainDirectory $InstallRoot
    $pointer = Join-Path $InstallRoot '.current-version'
    if (!(Test-Path -LiteralPath $pointer)) { return $InstallRoot }
    if ((Get-Item -LiteralPath $pointer).Length -gt 80) { throw 'Invalid active version pointer.' }
    $commit = (Get-Content -LiteralPath $pointer -Raw).Trim()
    if ($commit -cnotmatch '^[a-f0-9]{40}$') { throw 'Invalid active version pointer.' }
    $versions = Join-Path $InstallRoot '.versions'
    Assert-PlainDirectory $versions
    $active = Join-Path $versions $commit
    Assert-PlainDirectory $active
    return $active
}

function ConvertTo-NativeArgument([string]$Value) {
    # Windows CommandLineToArgvW quoting, including trailing backslashes and literal quotes.
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function Test-PortableRunning([string]$InstallRoot) {
    $prefix = [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\') + '\'
    foreach ($process in @(Get-Process -Name 'DeckCraft-Fork' -ErrorAction SilentlyContinue)) {
        try { $path = $process.Path } catch { throw 'Cannot check an existing DeckCraft process. Close it first.' }
        if (!$path -or $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
    return $false
}

function Receive-PortableFile([string]$Uri, [string]$Destination, [long]$Limit) {
    $address = [Uri]$Uri
    if ($address.Scheme -ne 'https' -or $address.Host -notin @('api.github.com', 'github.com')) {
        throw 'Unexpected update download address.'
    }
    $request = [Net.HttpWebRequest]::Create($address)
    $request.UserAgent = 'DeckCraft-Fork-Updater/1'
    $request.Timeout = 30000
    $request.ReadWriteTimeout = 30000
    $request.MaximumAutomaticRedirections = 5
    $response = $null; $inputStream = $null; $outputStream = $null
    try {
        $response = $request.GetResponse()
        if ($response.ResponseUri.Scheme -ne 'https' -or $response.ResponseUri.Host -notin @(
            'api.github.com', 'github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com')) {
            throw 'Unexpected update redirect.'
        }
        if ($response.ContentLength -gt $Limit) { throw 'Update download exceeds the size limit.' }
        $inputStream = $response.GetResponseStream()
        $outputStream = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew)
        $buffer = New-Object byte[] 65536
        [long]$total = 0
        $deadline = [DateTime]::UtcNow.AddMinutes(15)
        while (($read = $inputStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            $total += $read
            if ($total -gt $Limit -or [DateTime]::UtcNow -gt $deadline) { throw 'Update download exceeded its size or time limit.' }
            $outputStream.Write($buffer, 0, $read)
            if ($response.ContentLength -gt 0) {
                Write-Progress -Activity 'Downloading DeckCraft Fork' -Status ('{0:N1} MiB' -f ($total / 1MB)) -PercentComplete ([int](100 * $total / $response.ContentLength))
            }
        }
        if ($response.ContentLength -ge 0 -and $total -ne $response.ContentLength) { throw 'Incomplete update download.' }
    } finally {
        if ($outputStream) { $outputStream.Dispose() }
        if ($inputStream) { $inputStream.Dispose() }
        if ($response) { $response.Dispose() }
        Write-Progress -Activity 'Downloading DeckCraft Fork' -Completed
    }
}

function Expand-PortablePackage([string]$ZipPath, [string]$Destination, $Manifest, [string]$Commit) {
    if ($Commit -cnotmatch '^[a-f0-9]{40}$' -or $Manifest.schema -ne 1 -or
        $Manifest.repository -cne $script:PortableRepository -or $Manifest.commit -cne $Commit -or
        $Manifest.channel -cne 'main' -or $Manifest.sha256 -notmatch '^[a-fA-F0-9]{64}$') {
        throw 'Invalid update manifest.'
    }
    $zipItem = Get-Item -LiteralPath $ZipPath
    if ($zipItem.Length -gt 128MB -or $Manifest.size -ne $zipItem.Length) { throw 'Invalid update archive size.' }
    if ((Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash -ne $Manifest.sha256) { throw 'Update checksum mismatch.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
        if ($zip.Entries.Count -gt 32) { throw 'Too many update archive entries.' }
        $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$total = 0
        foreach ($entry in $zip.Entries) {
            # Windows PowerShell 5 Compress-Archive uses backslashes; normalize before
            # validation so traversal and duplicate aliases are rejected on either form.
            $name = $entry.FullName.Replace('\', '/')
            if ($name -ceq 'DeckCraft-Fork/') { continue }
            if ($name.StartsWith('DeckCraft-Fork/', [StringComparison]::Ordinal)) { $name = $name.Substring(15) }
            if ($name -cnotin $script:PortableFiles -or !$seen.Add($name) -or
                (($entry.ExternalAttributes -shr 16) -band 61440) -eq 40960) {
                throw 'Unexpected, duplicate or linked update archive entry.'
            }
            $total += $entry.Length
            if ($entry.Length -lt 0 -or $total -gt 256MB) { throw 'Expanded update exceeds the size limit.' }
            if (($name -ceq 'BUILD-INFO.json' -and $entry.Length -gt 16KB) -or
                ($name -cne 'DeckCraft-Fork.exe' -and $entry.Length -gt 2MB)) { throw 'Update metadata or script exceeds the size limit.' }
        }
        foreach ($required in $script:PortableFiles) {
            if (!$seen.Contains($required)) { throw "Update archive is missing $required" }
        }
        New-Item -ItemType Directory -Path $Destination -ErrorAction Stop | Out-Null
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('\', '/')
            if ($name -ceq 'DeckCraft-Fork/') { continue }
            if ($name.StartsWith('DeckCraft-Fork/', [StringComparison]::Ordinal)) { $name = $name.Substring(15) }
            $source = $entry.Open()
            $target = [IO.File]::Open((Join-Path $Destination $name), [IO.FileMode]::CreateNew)
            try {
                $buffer = New-Object byte[] 65536
                [long]$written = 0
                while (($read = $source.Read($buffer, 0, $buffer.Length)) -gt 0) {
                    $written += $read
                    if ($written -gt $entry.Length) { throw 'Invalid expanded entry length.' }
                    $target.Write($buffer, 0, $read)
                }
                if ($written -ne $entry.Length) { throw 'Truncated update entry.' }
            } finally { $target.Dispose(); $source.Dispose() }
        }
    } finally { $zip.Dispose() }
    $info = Get-Content -LiteralPath (Join-Path $Destination 'BUILD-INFO.json') -Raw | ConvertFrom-Json
    if ($info.repository -cne $script:PortableRepository -or $info.commit -cne $Commit) { throw 'Update build identity mismatch.' }
}

function Remove-PortablePackage([string]$Directory, [string]$Parent, [switch]$ManagedVersion) {
    # Delete only a flat directory consisting entirely of our package files. Preserve extras.
    $full = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
    if ([IO.Path]::GetDirectoryName($full) -ine [IO.Path]::GetFullPath($Parent).TrimEnd('\')) { throw 'Unsafe package cleanup path.' }
    Assert-PlainDirectory $Parent
    Assert-PlainDirectory $full
    if ($ManagedVersion) {
        $marker = Join-Path $full '.managed-version.json'
        if (!(Test-Path -LiteralPath $marker -PathType Leaf) -or (Get-Item -LiteralPath $marker).Length -gt 4KB) { return }
        $owner = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
        if ($owner.repository -cne $script:PortableRepository -or $owner.commit -cne [IO.Path]::GetFileName($full)) { return }
    }
    $items = @(Get-ChildItem -LiteralPath $full -Force)
    foreach ($item in $items) {
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Name -cnotin ($script:PortableFiles + '.managed-version.json')) {
            return # User files or unexpected content: keep the directory intact.
        }
    }
    foreach ($item in $items) { Remove-Item -LiteralPath $item.FullName -Force }
    Remove-Item -LiteralPath $full -Force
}

function Enable-PortableVersion([string]$InstallRoot, [string]$PackageDirectory, [string]$Commit) {
    if ($Commit -cnotmatch '^[a-f0-9]{40}$') { throw 'Invalid update commit.' }
    Assert-PlainDirectory $InstallRoot
    if (Test-PortableRunning $InstallRoot) { throw 'Save and close DeckCraft Fork before installing the update.' }
    $versions = Join-Path $InstallRoot '.versions'
    if (!(Test-Path -LiteralPath $versions)) { New-Item -ItemType Directory -Path $versions | Out-Null }
    Assert-PlainDirectory $versions
    $destination = Join-Path $versions $Commit
    if (Test-Path -LiteralPath $destination) {
        # A terminated updater may have moved the candidate without publishing its pointer.
        # Never remove the selected version or a directory we cannot prove we own.
        if ((Get-ActivePortableDirectory $InstallRoot) -ieq $destination) { throw 'This version is already active.' }
        Remove-PortablePackage $destination $versions -ManagedVersion
        if (Test-Path -LiteralPath $destination) { throw 'An existing version contains unowned or user-added files. It has been preserved.' }
    }
    # Persist ownership before the move, so every stranded candidate can be reconciled.
    @{repository=$script:PortableRepository;commit=$Commit} | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $PackageDirectory '.managed-version.json') -Encoding utf8
    Move-Item -LiteralPath $PackageDirectory -Destination $destination
    $pointer = Join-Path $InstallRoot '.current-version'
    $temporaryPointer = Join-Path $InstallRoot ('.current-' + [Guid]::NewGuid().ToString('N'))
    try {
        # Exercise native startup before switching the launcher. Failure keeps the old version.
        $process = Start-Process -FilePath (Join-Path $destination 'DeckCraft-Fork.exe') -ArgumentList '--version' -WindowStyle Hidden -PassThru
        if (!$process.WaitForExit(15000)) { $process.Kill(); throw 'Updated executable did not start.' }
        if ($process.ExitCode -ne 0) { throw 'Updated executable failed its startup check.' }
        if (Test-PortableRunning $InstallRoot) { throw 'DeckCraft was opened during the update. Close it and retry.' }
        [IO.File]::WriteAllText($temporaryPointer, $Commit, [Text.Encoding]::ASCII)
        if (Test-Path -LiteralPath $pointer) { [IO.File]::Replace($temporaryPointer, $pointer, [NullString]::Value) }
        else { [IO.File]::Move($temporaryPointer, $pointer) }
    } catch {
        if (Test-Path -LiteralPath $temporaryPointer) { Remove-Item -LiteralPath $temporaryPointer -Force }
        Remove-PortablePackage $destination $versions
        throw
    }
    # Old downloaded versions are disposable only after activation. Keep the original base
    # package, all settings/recovery and any directory containing user-added files.
    foreach ($old in @(Get-ChildItem -LiteralPath $versions -Directory -Force)) {
        if ($old.Name -cmatch '^[a-f0-9]{40}$' -and $old.Name -cne $Commit) {
            try { Remove-PortablePackage $old.FullName $versions -ManagedVersion } catch { Write-Warning 'An old version could not be cleaned; the update is installed.' }
        }
    }
}

function Remove-PortableWork([string]$Directory, [string]$InstallRoot) {
    $full = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
    if ([IO.Path]::GetDirectoryName($full) -ine [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\') -or
        [IO.Path]::GetFileName($full) -cnotmatch '^\.update-[a-f0-9]{32}$') { throw 'Unsafe update cleanup path.' }
    Assert-PlainDirectory $InstallRoot
    Assert-PlainDirectory $full
    $marker = Join-Path $full '.owner.json'
    if (!(Test-Path -LiteralPath $marker -PathType Leaf) -or (Get-Item -LiteralPath $marker).Length -gt 4KB) { return }
    $owner = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
    if ($owner.repository -cne $script:PortableRepository -or $owner.directory -cne [IO.Path]::GetFileName($full)) { return }
    $files = @('releases.json', 'update.json', 'package.zip', '.owner.json')
    foreach ($item in @(Get-ChildItem -LiteralPath $full -Force)) {
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            ($item.Name -cnotin ($files + 'package')) -or ($item.PSIsContainer -and $item.Name -cne 'package')) { return }
    }
    $package = Join-Path $full 'package'
    if (Test-Path -LiteralPath $package) {
        Remove-PortablePackage $package $full
        if (Test-Path -LiteralPath $package) { return }
    }
    foreach ($file in $files) {
        $path = Join-Path $full $file
        if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force }
    }
    Remove-Item -LiteralPath $full
}

function Clear-PortableInterruptedDownloads([string]$InstallRoot) {
    # Caller holds .update.lock, so an owned work directory cannot belong to a live update.
    Assert-PlainDirectory $InstallRoot
    foreach ($stale in @(Get-ChildItem -LiteralPath $InstallRoot -Directory -Force)) {
        if ($stale.Name -cmatch '^\.update-[a-f0-9]{32}$') {
            try { Remove-PortableWork $stale.FullName $InstallRoot }
            catch { Write-Warning 'An abandoned update download could not be cleaned.' }
        }
    }
}
