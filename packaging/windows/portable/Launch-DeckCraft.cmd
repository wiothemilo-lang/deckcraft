@echo off
setlocal
if not exist "%~dp0DeckCraft-Fork.exe" (
    echo DeckCraft-Fork.exe is missing. Extract the complete portable ZIP first.
    pause
    exit /b 1
)
rem Only the child process receives this path. Keep fork settings and recovery
rem separate from the original app without changing the user's environment.
set "APPDATA=%LOCALAPPDATA%\DeckCraft-Fork"
start "" "%~dp0DeckCraft-Fork.exe" %*
